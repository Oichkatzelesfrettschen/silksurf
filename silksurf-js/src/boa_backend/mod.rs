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
    collections::HashMap,
    fmt::Write as _,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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

mod css_object;
mod dom_bridge;
mod event_dispatch;
mod net_queue;

const HOST_CALLBACKS_REGISTRY: &str = "__silksurfHostCallbacks";
const DEFAULT_HOST_CALLBACK_BUDGET: usize = 256;
const TRACE_HOST_CALLBACKS_ENV: &str = "SILKSURF_TRACE_HOST_CALLBACKS";

type HostSchedulerRef = Rc<RefCell<HostScheduler>>;

/// Async-test completion state recorded by the `$DONE` host function.
///
/// `$DONE()` (or a falsy argument) signals success; a truthy argument signals
/// failure with a message; a second call is itself a failure. This mirrors the
/// test262 async convention and the pattern benchmark harnesses use to signal
/// the end of an asynchronous run through the host job/timer queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsyncCompletion {
    /// `$DONE` has not been called yet.
    Pending,
    /// `$DONE()` was called with no error.
    Passed,
    /// `$DONE(error)` was called, or `$DONE` was called more than once.
    Failed(String),
}

#[derive(Default)]
struct AsyncDoneCell {
    called: bool,
    result: Option<Result<(), String>>,
}

type AsyncDoneRef = Rc<RefCell<AsyncDoneCell>>;

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

/// A repeating timer: `next_due` advances by `period` each firing.
struct IntervalEntry {
    id: u32,
    period: Duration,
    next_due: Instant,
}

#[derive(Default)]
struct HostScheduler {
    next_id: u32,
    timeout_callbacks: Vec<(u32, Instant)>,
    interval_callbacks: Vec<IntervalEntry>,
    animation_frame_callbacks: Vec<u32>,
}

impl HostScheduler {
    fn new() -> Self {
        Self {
            next_id: 1,
            timeout_callbacks: Vec::new(),
            interval_callbacks: Vec::new(),
            animation_frame_callbacks: Vec::new(),
        }
    }

    fn schedule(&mut self, queue: HostCallbackQueue, delay: Duration) -> u32 {
        let id = self.next_callback_id();
        let now = Instant::now();
        match queue {
            HostCallbackQueue::Timeout => self.timeout_callbacks.push((id, now + delay)),
            HostCallbackQueue::Interval => self.interval_callbacks.push(IntervalEntry {
                id,
                period: delay,
                next_due: now + delay,
            }),
            HostCallbackQueue::AnimationFrame => self.animation_frame_callbacks.push(id),
        }
        id
    }

    fn cancel(&mut self, id: u32) {
        self.timeout_callbacks
            .retain(|(callback_id, _)| *callback_id != id);
        self.interval_callbacks.retain(|entry| entry.id != id);
        self.animation_frame_callbacks
            .retain(|callback_id| *callback_id != id);
    }

    fn has_pending_callbacks(&self) -> bool {
        !self.timeout_callbacks.is_empty()
            || !self.interval_callbacks.is_empty()
            || !self.animation_frame_callbacks.is_empty()
    }

    /// Earliest instant at which a scheduled timer becomes runnable.
    ///
    /// Pending animation-frame callbacks report `now`: they are frame-paced
    /// by the presenter, so any waiting event loop should wake immediately.
    fn next_deadline(&self) -> Option<Instant> {
        let mut deadline: Option<Instant> = None;
        let mut consider = |candidate: Instant| {
            deadline = Some(deadline.map_or(candidate, |current| current.min(candidate)));
        };
        for &(_, due) in &self.timeout_callbacks {
            consider(due);
        }
        for entry in &self.interval_callbacks {
            consider(entry.next_due);
        }
        if !self.animation_frame_callbacks.is_empty() {
            consider(Instant::now());
        }
        deadline
    }

    /// Take up to `max_callbacks` timers whose deadline has passed,
    /// earliest deadline first. Due intervals re-arm by their period.
    fn take_timer_callbacks(&mut self, max_callbacks: usize) -> Vec<ScheduledHostCallback> {
        let now = Instant::now();
        let mut due: Vec<(Instant, ScheduledHostCallback)> = Vec::new();
        self.timeout_callbacks.retain(|&(id, deadline)| {
            if deadline <= now {
                due.push((
                    deadline,
                    ScheduledHostCallback {
                        id,
                        repeat: false,
                        queue: HostCallbackQueue::Timeout,
                    },
                ));
                false
            } else {
                true
            }
        });
        for entry in &mut self.interval_callbacks {
            if entry.next_due <= now {
                due.push((
                    entry.next_due,
                    ScheduledHostCallback {
                        id: entry.id,
                        repeat: true,
                        queue: HostCallbackQueue::Interval,
                    },
                ));
                entry.next_due = now + entry.period;
            }
        }
        due.sort_by_key(|&(deadline, _)| deadline);
        let overflow: Vec<ScheduledHostCallback> = due
            .split_off(max_callbacks.min(due.len()))
            .into_iter()
            .map(|(_, callback)| callback)
            .collect();
        // Budget-clipped one-shot timeouts go back on the queue as
        // immediately-due entries; intervals re-fire from their next_due.
        for callback in overflow {
            if !callback.repeat {
                self.timeout_callbacks.push((callback.id, now));
            }
        }
        due.into_iter().map(|(_, callback)| callback).collect()
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
type CookieJar = Arc<Mutex<silksurf_net::cookie::PartitionedCookieStore>>;

type StorageDirtyFlag = Rc<std::cell::Cell<bool>>;

/// Install localStorage/sessionStorage; returns the localStorage map and its
/// dirty flag so the embedder can preload persisted entries and flush writes.
fn install_storage_objects(ctx: &mut Context) -> (StorageMap, StorageDirtyFlag) {
    let dirty = Rc::new(std::cell::Cell::new(false));
    let (local_storage, local_map) = storage_object(ctx, Some(&dirty));
    let (session_storage, _session_map) = storage_object(ctx, None);
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
    (local_map, dirty)
}

fn storage_object(ctx: &mut Context, dirty: Option<&StorageDirtyFlag>) -> (JsObject, StorageMap) {
    let storage = Rc::new(RefCell::new(HashMap::new()));
    let length_getter =
        FunctionObjectBuilder::new(ctx.realm(), storage_length_native(&storage)).build();

    let object = ObjectInitializer::new(ctx)
        .accessor(
            js_string!("length"),
            Some(length_getter),
            None,
            Attribute::CONFIGURABLE | Attribute::ENUMERABLE,
        )
        .function(storage_get_item_native(&storage), js_string!("getItem"), 1)
        .function(
            storage_set_item_native(&storage, dirty),
            js_string!("setItem"),
            2,
        )
        .function(
            storage_remove_item_native(&storage, dirty),
            js_string!("removeItem"),
            1,
        )
        .function(
            storage_clear_native(&storage, dirty),
            js_string!("clear"),
            0,
        )
        .function(storage_key_native(&storage), js_string!("key"), 1)
        .build();
    (object, storage)
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

fn storage_set_item_native(
    storage: &StorageMap,
    dirty: Option<&StorageDirtyFlag>,
) -> NativeFunction {
    let storage = Rc::clone(storage);
    let dirty = dirty.map(Rc::clone);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = storage_string_arg(args.first(), ctx)?;
            let value = storage_string_arg(args.get(1), ctx)?;
            storage.borrow_mut().insert(key, value);
            if let Some(flag) = &dirty {
                flag.set(true);
            }
            Ok(JsValue::undefined())
        })
    }
}

fn storage_remove_item_native(
    storage: &StorageMap,
    dirty: Option<&StorageDirtyFlag>,
) -> NativeFunction {
    let storage = Rc::clone(storage);
    let dirty = dirty.map(Rc::clone);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let key = storage_string_arg(args.first(), ctx)?;
            storage.borrow_mut().remove(&key);
            if let Some(flag) = &dirty {
                flag.set(true);
            }
            Ok(JsValue::undefined())
        })
    }
}

fn storage_clear_native(storage: &StorageMap, dirty: Option<&StorageDirtyFlag>) -> NativeFunction {
    let storage = Rc::clone(storage);
    let dirty = dirty.map(Rc::clone);
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            storage.borrow_mut().clear();
            if let Some(flag) = &dirty {
                flag.set(true);
            }
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
    Arc::new(Mutex::new(
        silksurf_net::cookie::PartitionedCookieStore::new(),
    ))
}

fn document_cookie_getter(
    ctx: &mut Context,
    jar: &CookieJar,
    top_level_site: &str,
    host: &str,
) -> JsFunction {
    FunctionObjectBuilder::new(
        ctx.realm(),
        document_cookie_get_native(jar, top_level_site, host),
    )
    .build()
}

fn document_cookie_setter(
    ctx: &mut Context,
    jar: &CookieJar,
    top_level_site: &str,
    host: &str,
) -> JsFunction {
    FunctionObjectBuilder::new(
        ctx.realm(),
        document_cookie_set_native(jar, top_level_site, host),
    )
    .build()
}

fn document_cookie_get_native(jar: &CookieJar, top_level_site: &str, host: &str) -> NativeFunction {
    let jar = Arc::clone(jar);
    let top_level_site = top_level_site.to_string();
    let host = host.to_string();
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let header = jar
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .document_cookie_string(&top_level_site, &host, silksurf_net::cookie::now_unix());
            Ok(JsValue::from(JsString::from(header.as_str())))
        })
    }
}

fn document_cookie_set_native(jar: &CookieJar, top_level_site: &str, host: &str) -> NativeFunction {
    let jar = Arc::clone(jar);
    let top_level_site = top_level_site.to_string();
    let host = host.to_string();
    // SAFETY: Boa stores the native closure with owned Rust captures for the JS function lifetime.

    unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let value = storage_string_arg(args.first(), ctx)?;
            jar.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .set_document_cookie(
                    &top_level_site,
                    &host,
                    value.as_str(),
                    silksurf_net::cookie::now_unix(),
                );
            Ok(JsValue::undefined())
        })
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
    async_done: AsyncDoneRef,
    start_time: Instant,
    /// Live document handle; present only for contexts built with a DOM
    /// bridge (`with_dom` / `with_dom_and_cookies`). Event dispatch needs it
    /// for ancestor-path snapshots.
    dom: Option<Arc<Mutex<silksurf_dom::Dom>>>,
    /// Network completion queue: fetch worker threads report here and
    /// `run_host_callbacks` settles the parked promises.
    net: net_queue::NetQueue,
    /// Live WebSocket sessions; `run_host_callbacks` pumps their incoming
    /// frames into on{open,message,error,close} handlers.
    ws_sessions: WsSessionsRef,
    /// Live `EventSource` subscriptions, pumped the same way.
    sse_subscriptions: SseSubscriptionsRef,
    /// Same-document navigations queued by history.pushState/replaceState.
    history_intents: HistoryIntentsRef,
    /// localStorage backing map; the embedder preloads persisted entries
    /// and flushes writes signaled by `storage_dirty`.
    local_storage: StorageMap,
    storage_dirty: StorageDirtyFlag,
    /// Viewport dimensions backing matchMedia (and future viewport units).
    viewport: ViewportRef,
}

/// Extra payload field on a synthetic event object.
#[derive(Debug, Clone)]
pub enum SyntheticField {
    Number(f64),
    Text(String),
    Flag(bool),
}

/// A host-synthesized DOM event (a real user click or keystroke, as opposed
/// to a script-created `dispatchEvent` argument). Dispatched events carry
/// `isTrusted: true`.
#[derive(Debug, Clone)]
pub struct SyntheticEvent {
    pub event_type: String,
    pub bubbles: bool,
    pub cancelable: bool,
    pub fields: Vec<(String, SyntheticField)>,
}

impl SyntheticEvent {
    #[must_use]
    pub fn new(event_type: &str, bubbles: bool, cancelable: bool) -> Self {
        Self {
            event_type: event_type.to_string(),
            bubbles,
            cancelable,
            fields: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_field(mut self, name: &str, value: SyntheticField) -> Self {
        self.fields.push((name.to_string(), value));
        self
    }
}

/// Result of dispatching a synthetic event through the listener tree.
#[derive(Debug, Clone, Copy)]
pub struct DispatchOutcome {
    /// True when a listener called `preventDefault()` on a cancelable event;
    /// the embedder must then suppress the native default action.
    pub default_prevented: bool,
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
        // Asynchronous execution: the request runs on a worker thread and the
        // promise settles when run_host_callbacks drains the completion queue.
        let net = net_queue::NetQueue::new();
        install_async_fetch(&mut ctx, &net.shared);
        let ws_sessions: WsSessionsRef = Rc::new(RefCell::new(Vec::new()));
        install_websocket(&mut ctx, &ws_sessions);
        let sse_subscriptions: SseSubscriptionsRef = Rc::new(RefCell::new(Vec::new()));
        install_event_source(&mut ctx, &sse_subscriptions);
        install_stream_constructors(&mut ctx);
        install_crypto(&mut ctx);
        install_abort_api(&mut ctx);
        install_xml_http_request(&mut ctx);

        // -- document stub ----------------------------------------------------
        // getElementById / querySelector / querySelectorAll return null until
        // the full DOM bridge (NativeObject-backed NodeId handles) is wired in.
        let cookie_jar = new_cookie_jar();
        let cookie_getter = document_cookie_getter(&mut ctx, &cookie_jar, "", "");
        let cookie_setter = document_cookie_setter(&mut ctx, &cookie_jar, "", "");
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

        // -- queueMicrotask / matchMedia / history ------------------------------
        let history_intents: HistoryIntentsRef = Rc::new(RefCell::new(Vec::new()));
        let viewport = Rc::new(std::cell::Cell::new((1280.0_f32, 720.0_f32)));
        install_match_media_native(&mut ctx, &viewport);
        install_history_intent_native(&mut ctx, &history_intents);
        let bootstrap = r"
            globalThis.queueMicrotask = function (cb) {
                Promise.resolve().then(function () { cb(); });
            };
            globalThis.matchMedia = function (query) {
                query = String(query);
                return {
                    media: query,
                    matches: __silksurfMatchMedia(query),
                    onchange: null,
                    addEventListener: function () {},
                    removeEventListener: function () {},
                    addListener: function () {},
                    removeListener: function () {}
                };
            };
            globalThis.history = {
                state: null,
                length: 1,
                pushState: function (state, _title, url) {
                    this.state = state;
                    this.length += 1;
                    if (url !== undefined && url !== null) {
                        location.href = String(url);
                    }
                    __silksurfHistoryIntent(false, url === undefined || url === null ? '' : String(url),
                        state === undefined ? 'null' : JSON.stringify(state) || 'null');
                },
                replaceState: function (state, _title, url) {
                    this.state = state;
                    if (url !== undefined && url !== null) {
                        location.href = String(url);
                    }
                    __silksurfHistoryIntent(true, url === undefined || url === null ? '' : String(url),
                        state === undefined ? 'null' : JSON.stringify(state) || 'null');
                },
                back: function () {},
                forward: function () {},
                go: function () {}
            };
        ";
        ctx.eval(Source::from_bytes(bootstrap.as_bytes()))
            // UNWRAP-OK: the bootstrap script is a compile-time constant that parses.
            .expect("host bootstrap script evaluates");

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

        // -- performance ------------------------------------------------------
        // now() returns fractional milliseconds since a process-wide monotonic
        // epoch, so benchmark self-timing measures real elapsed time; mark()
        // and measure() remain no-ops.
        let performance = ObjectInitializer::new(&mut ctx)
            .function(
                NativeFunction::from_fn_ptr(performance_now),
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

        let (local_storage, storage_dirty) = install_storage_objects(&mut ctx);
        let async_done = Rc::new(RefCell::new(AsyncDoneCell::default()));
        install_async_done(&mut ctx, &async_done);

        Self {
            ctx,
            module_loader,
            scheduler,
            async_done,
            start_time: Instant::now(),
            dom: None,
            net,
            ws_sessions,
            sse_subscriptions,
            history_intents,
            viewport,
            local_storage,
            storage_dirty,
        }
    }

    /// Seed localStorage with persisted entries (before page scripts run).
    pub fn preload_local_storage(&mut self, entries: HashMap<String, String>) {
        self.local_storage.borrow_mut().extend(entries);
        self.storage_dirty.set(false);
    }

    /// Snapshot localStorage when a write happened since the last take.
    pub fn take_local_storage_if_dirty(&mut self) -> Option<HashMap<String, String>> {
        if !self.storage_dirty.get() {
            return None;
        }
        self.storage_dirty.set(false);
        Some(self.local_storage.borrow().clone())
    }

    /// Update the viewport dimensions matchMedia evaluates against.
    /// The embedder calls this on window resize.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport.set((width, height));
    }

    /// Drain the same-document navigations queued by history.pushState and
    /// replaceState since the last call.
    pub fn take_history_intents(&mut self) -> Vec<HistoryIntent> {
        std::mem::take(&mut *self.history_intents.borrow_mut())
    }

    /// Install the computed-style provider backing `getComputedStyle`.
    ///
    /// The provider maps (node, kebab-case property) to a serialized value,
    /// computed fresh on demand -- so a `style.color` write made earlier in
    /// the same script is visible immediately. Properties the provider does
    /// not serialize return the empty string, matching the CSSOM contract
    /// for unsupported properties.
    pub fn set_computed_style_provider(&mut self, provider: ComputedStyleProvider) {
        // SAFETY: the capture is an Rc closure over app data, no GC pointers.
        let native = unsafe {
            NativeFunction::from_closure(move |_this, args, ctx| {
                let node = args
                    .first()
                    .map(|value| value.to_u32(ctx))
                    .transpose()?
                    .unwrap_or(0);
                let prop = args
                    .get(1)
                    .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                    .transpose()?
                    .unwrap_or_default();
                let kebab = css_object::camel_to_kebab(&prop);
                let value = provider(silksurf_dom::NodeId::from_raw(node as usize), &kebab)
                    .unwrap_or_default();
                Ok(JsValue::from(JsString::from(value.as_str())))
            })
        };
        let _ =
            self.ctx
                .register_global_callable(js_string!("__silksurfComputedStyleGet"), 2, native);
        let bootstrap = r"
            globalThis.getComputedStyle = function (el) {
                var nodeId = el && typeof el.nodeId === 'number' ? el.nodeId : 0;
                return new Proxy({}, {
                    get: function (t, prop) {
                        if (prop === 'getPropertyValue') {
                            return function (name) {
                                return __silksurfComputedStyleGet(nodeId, name);
                            };
                        }
                        if (typeof prop !== 'string') { return undefined; }
                        return __silksurfComputedStyleGet(nodeId, prop);
                    }
                });
            };
        ";
        if let Err(err) = self.ctx.eval(Source::from_bytes(bootstrap.as_bytes())) {
            eprintln!("silksurf-js: getComputedStyle bootstrap failed: {err}");
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

    /// Return true when host timer or frame callbacks are ready to run, or
    /// network requests are still in flight.
    #[must_use]
    pub fn has_pending_host_callbacks(&self) -> bool {
        self.scheduler.borrow().has_pending_callbacks()
            || self.net.in_flight() > 0
            || !self.ws_sessions.borrow().is_empty()
            || !self.sse_subscriptions.borrow().is_empty()
    }

    /// Earliest instant at which a scheduled host callback becomes due.
    ///
    /// Event loops use this to sleep exactly until the next `setTimeout` or
    /// `setInterval` deadline instead of polling; `None` means nothing is
    /// scheduled and the loop can wait for external events alone.
    #[must_use]
    pub fn next_host_callback_deadline(&self) -> Option<Instant> {
        let timer_deadline = self.scheduler.borrow().next_deadline();
        // In-flight network work polls at a fixed cadence; the completion
        // channel cannot wake the winit loop directly (a real waker via
        // EventLoopProxy is a named follow-up in the SPA roadmap).
        let live_push_channels =
            !self.ws_sessions.borrow().is_empty() || !self.sse_subscriptions.borrow().is_empty();
        let net_deadline = (self.net.in_flight() > 0 || live_push_channels)
            .then(|| Instant::now() + std::time::Duration::from_millis(10));
        match (timer_deadline, net_deadline) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (deadline, None) | (None, deadline) => deadline,
        }
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
        ran += self.drain_net_completions()?;
        ran += self.drain_ws_events()?;
        ran += self.drain_sse_events()?;
        let jobs_start = Instant::now();
        self.run_pending_jobs();
        trace_host_callback_jobs(trace_callbacks, jobs_start.elapsed());
        Ok(ran)
    }

    /// Network requests spawned but not yet drained. One-shot embedders
    /// (headless render, CLI runners) use this to pump briefly until page
    /// fetches settle instead of sleeping on timer deadlines.
    #[must_use]
    pub fn inflight_network_requests(&self) -> usize {
        self.net.in_flight()
    }

    /// Pump every live WebSocket session's incoming frames into its JS
    /// instance's on{open,message,error,close} handlers. Sessions whose
    /// socket closed are unbound and their JS registry slot cleared.
    fn drain_ws_events(&mut self) -> Result<usize, String> {
        // Snapshot events first: handler invocation re-enters the context and
        // must not hold the sessions borrow.
        let mut events: Vec<(u64, silksurf_net::WsIncoming)> = Vec::new();
        let mut closed_keys: Vec<u64> = Vec::new();
        {
            let sessions = self.ws_sessions.borrow();
            for binding in sessions.iter() {
                while let Some(event) = binding.session.try_next() {
                    if matches!(event, silksurf_net::WsIncoming::Closed) {
                        closed_keys.push(binding.key);
                    }
                    events.push((binding.key, event));
                }
            }
        }
        let fired = events.len();
        for (key, event) in events {
            let Some(socket) = ws_instance_object(key, &mut self.ctx).map_err(|e| e.to_string())?
            else {
                continue;
            };
            let result: boa_engine::JsResult<()> = (|| {
                match event {
                    silksurf_net::WsIncoming::Open => {
                        socket.set(js_string!("readyState"), 1_u32, false, &mut self.ctx)?;
                        let open_event = ObjectInitializer::new(&mut self.ctx)
                            .property(js_string!("type"), js_string!("open"), Attribute::all())
                            .build();
                        call_websocket_handler(
                            &socket,
                            js_string!("onopen"),
                            open_event.into(),
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::WsIncoming::Text(text) => {
                        socket.set(
                            js_string!("lastMessage"),
                            JsString::from(text.as_str()),
                            false,
                            &mut self.ctx,
                        )?;
                        let event = websocket_message_event(text.as_str(), &mut self.ctx);
                        call_websocket_handler(
                            &socket,
                            js_string!("onmessage"),
                            event,
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::WsIncoming::Binary(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        let event = websocket_message_event(text.as_str(), &mut self.ctx);
                        call_websocket_handler(
                            &socket,
                            js_string!("onmessage"),
                            event,
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::WsIncoming::Error(message) => {
                        socket.set(
                            js_string!("lastError"),
                            JsString::from(message.as_str()),
                            false,
                            &mut self.ctx,
                        )?;
                        let event = websocket_error_event(message.as_str(), &mut self.ctx);
                        call_websocket_handler(
                            &socket,
                            js_string!("onerror"),
                            event,
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::WsIncoming::Closed => {
                        socket.set(js_string!("readyState"), 3_u32, false, &mut self.ctx)?;
                        let close_event = ObjectInitializer::new(&mut self.ctx)
                            .property(js_string!("type"), js_string!("close"), Attribute::all())
                            .build();
                        call_websocket_handler(
                            &socket,
                            js_string!("onclose"),
                            close_event.into(),
                            &mut self.ctx,
                        )?;
                    }
                }
                Ok(())
            })();
            result.map_err(|err| format!("{err}"))?;
        }
        if !closed_keys.is_empty() {
            self.ws_sessions
                .borrow_mut()
                .retain(|binding| !closed_keys.contains(&binding.key));
            for key in closed_keys {
                ws_drop_instance(key, &mut self.ctx).map_err(|err| format!("{err}"))?;
            }
        }
        Ok(fired)
    }

    /// Pump every live `EventSource` subscription's events into its JS
    /// instance's handlers. Named events dispatch to `on<type>` when such a
    /// handler exists, otherwise to `onmessage`.
    fn drain_sse_events(&mut self) -> Result<usize, String> {
        let mut events: Vec<(u64, silksurf_net::SseIncoming)> = Vec::new();
        let mut closed_keys: Vec<u64> = Vec::new();
        {
            let subscriptions = self.sse_subscriptions.borrow();
            for binding in subscriptions.iter() {
                while let Some(event) = binding.subscription.try_next() {
                    if matches!(event, silksurf_net::SseIncoming::Closed) {
                        closed_keys.push(binding.key);
                    }
                    events.push((binding.key, event));
                }
            }
        }
        let fired = events.len();
        for (key, event) in events {
            let Some(source) =
                sse_instance_object(key, &mut self.ctx).map_err(|e| e.to_string())?
            else {
                continue;
            };
            let result: boa_engine::JsResult<()> = (|| {
                match event {
                    silksurf_net::SseIncoming::Open => {
                        source.set(js_string!("readyState"), 1_u32, false, &mut self.ctx)?;
                        let open_event = ObjectInitializer::new(&mut self.ctx)
                            .property(js_string!("type"), js_string!("open"), Attribute::all())
                            .build();
                        call_websocket_handler(
                            &source,
                            js_string!("onopen"),
                            open_event.into(),
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::SseIncoming::Event(sse_event) => {
                        let event_object = ObjectInitializer::new(&mut self.ctx)
                            .property(
                                js_string!("type"),
                                JsString::from(sse_event.event_type.as_str()),
                                Attribute::all(),
                            )
                            .property(
                                js_string!("data"),
                                JsString::from(sse_event.data.as_str()),
                                Attribute::all(),
                            )
                            .property(
                                js_string!("lastEventId"),
                                JsString::from(sse_event.id.unwrap_or_default().as_str()),
                                Attribute::all(),
                            )
                            .build();
                        let named = JsString::from(format!("on{}", sse_event.event_type).as_str());
                        let has_named = source
                            .get(named.clone(), &mut self.ctx)?
                            .as_object()
                            .is_some_and(|handler| handler.is_callable());
                        let handler_name = if has_named {
                            named
                        } else {
                            js_string!("onmessage")
                        };
                        call_websocket_handler(
                            &source,
                            handler_name,
                            event_object.into(),
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::SseIncoming::Error(message) => {
                        let event = websocket_error_event(message.as_str(), &mut self.ctx);
                        call_websocket_handler(
                            &source,
                            js_string!("onerror"),
                            event,
                            &mut self.ctx,
                        )?;
                    }
                    silksurf_net::SseIncoming::Closed => {
                        source.set(js_string!("readyState"), 2_u32, false, &mut self.ctx)?;
                    }
                }
                Ok(())
            })();
            result.map_err(|err| format!("{err}"))?;
        }
        if !closed_keys.is_empty() {
            self.sse_subscriptions
                .borrow_mut()
                .retain(|binding| !closed_keys.contains(&binding.key));
            for key in closed_keys {
                sse_drop_instance(key, &mut self.ctx).map_err(|err| format!("{err}"))?;
            }
        }
        Ok(fired)
    }

    /// Settle promises for every network completion that has arrived.
    fn drain_net_completions(&mut self) -> Result<usize, String> {
        let completions = self.net.drain();
        if completions.is_empty() {
            return Ok(0);
        }
        let mut settled = 0;
        for completion in completions {
            let resolvers =
                take_net_resolvers(completion.id, &mut self.ctx).map_err(|err| format!("{err}"))?;
            let Some((resolve, reject)) = resolvers else {
                continue;
            };
            let (function, argument) = match completion.payload {
                net_queue::NetPayload::Response(response) => {
                    (resolve, build_response_object(response, &mut self.ctx))
                }
                net_queue::NetPayload::Error(message) => {
                    let error = JsNativeError::error().with_message(message);
                    (
                        reject,
                        boa_engine::JsError::from(error).to_opaque(&mut self.ctx),
                    )
                }
            };
            if let Some(callable) = function.as_callable() {
                callable
                    .call(&JsValue::undefined(), &[argument], &mut self.ctx)
                    .map_err(|err| format!("{err}"))?;
                settled += 1;
            }
        }
        Ok(settled)
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
        dom_bridge::install_document(dom, &mut ctx.ctx, &new_cookie_jar(), "", "");
        ctx.dom = Some(Arc::clone(dom));
        ctx
    }

    /// Build a context with the DOM bridge and a shared cookie jar scoped to the
    /// document `host`, so `document.cookie` reads and writes the same store the
    /// HTTP client uses for that host. This is how cookies round-trip between
    /// network responses and script. An empty `host` leaves document.cookie
    /// unscoped (it matches every cookie in the jar).
    #[must_use]
    pub fn with_dom_and_cookies(
        dom: &Arc<Mutex<silksurf_dom::Dom>>,
        cookie_jar: &Arc<Mutex<silksurf_net::cookie::PartitionedCookieStore>>,
        top_level_site: &str,
        host: &str,
    ) -> Self {
        let mut ctx = Self::new();
        dom_bridge::install_document(dom, &mut ctx.ctx, cookie_jar, top_level_site, host);
        ctx.dom = Some(Arc::clone(dom));
        ctx
    }

    /// Dispatch a host-synthesized event at `target` with full
    /// capture/target/bubble propagation, then drain microtasks -- the same
    /// post-tick cadence the host scheduler uses.
    ///
    /// Returns an error string when this context has no DOM bridge installed.
    pub fn dispatch_dom_event(
        &mut self,
        target: silksurf_dom::NodeId,
        event: &SyntheticEvent,
    ) -> Result<DispatchOutcome, String> {
        let dom = self
            .dom
            .clone()
            .ok_or_else(|| "dispatch_dom_event: context has no DOM bridge".to_string())?;
        let event_object = event_dispatch::build_event_object(
            event.event_type.as_str(),
            event.bubbles,
            event.cancelable,
            true,
            &mut self.ctx,
        );
        for (name, value) in &event.fields {
            let js_value = match value {
                SyntheticField::Number(n) => JsValue::from(*n),
                SyntheticField::Text(s) => JsValue::from(JsString::from(s.as_str())),
                SyntheticField::Flag(b) => JsValue::from(*b),
            };
            event_object
                .set(
                    JsString::from(name.as_str()),
                    js_value,
                    false,
                    &mut self.ctx,
                )
                .map_err(|err| format!("{err}"))?;
        }
        let target_value = dom_bridge::node_to_js_object(&dom, target, &mut self.ctx);
        let proceed = event_dispatch::propagate_event(
            &dom,
            target,
            &target_value,
            &event_object,
            &mut self.ctx,
        )
        .map_err(|err| format!("{err}"))?;
        self.run_pending_jobs();
        Ok(DispatchOutcome {
            default_prevented: !proceed,
        })
    }

    /// True when any listener for `event_type` is registered on any node.
    /// Embedders check this before synthesizing input events so pages without
    /// listeners pay no dispatch cost.
    pub fn has_dom_listeners(&mut self, event_type: &str) -> bool {
        event_dispatch::any_listener_for_type(event_type, &mut self.ctx).unwrap_or(false)
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

    /// Current async-completion state signaled through `$DONE`.
    #[must_use]
    pub fn async_completion(&self) -> AsyncCompletion {
        match &self.async_done.borrow().result {
            None => AsyncCompletion::Pending,
            Some(Ok(())) => AsyncCompletion::Passed,
            Some(Err(message)) => AsyncCompletion::Failed(message.clone()),
        }
    }

    /// Clear the `$DONE` state so the context can drive another async run.
    pub fn reset_async_completion(&mut self) {
        let mut cell = self.async_done.borrow_mut();
        cell.called = false;
        cell.result = None;
    }

    /// Pump the microtask queue and host timer callbacks until `$DONE` fires or
    /// no scheduled work remains, waiting out timer deadlines up to `max_wall`.
    ///
    /// Returns the final `AsyncCompletion`: `Pending` means the script neither
    /// called `$DONE` nor left runnable work (or the wall-clock budget expired).
    /// This is the synchronous driver an embedder without a live event loop
    /// uses to run a promise/`setTimeout`-based async test to completion.
    pub fn drive_until_done(&mut self, max_wall: Duration) -> AsyncCompletion {
        let overall_deadline = Instant::now() + max_wall;
        loop {
            // Drain promise reactions from the last eval/callback, then run any
            // due timers (which itself drains the jobs they enqueue).
            self.run_pending_jobs();
            let _ = self.run_ready_host_callbacks();
            if !matches!(self.async_completion(), AsyncCompletion::Pending) {
                return self.async_completion();
            }
            // With no pending timers, nothing further can call $DONE.
            if !self.has_pending_host_callbacks() {
                return AsyncCompletion::Pending;
            }
            let now = Instant::now();
            if now >= overall_deadline {
                return AsyncCompletion::Pending;
            }
            match self.next_host_callback_deadline() {
                Some(next) if next > now => {
                    std::thread::sleep((next - now).min(overall_deadline - now));
                }
                Some(_) => {}
                None => return AsyncCompletion::Pending,
            }
        }
    }
}

/// Install the `$DONE(error)` host function recording async-test completion.
fn install_async_done(ctx: &mut Context, done: &AsyncDoneRef) {
    let cell = Rc::clone(done);
    // SAFETY: the closure captures an Rc<RefCell<AsyncDoneCell>>, which holds no
    // Boa GC pointers, so the native function needs no trace hook.
    let done_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let outcome = match args.first() {
                Some(value) if value.to_boolean() => Err(done_error_message(value, ctx)),
                _ => Ok(()),
            };
            let mut cell = cell.borrow_mut();
            if cell.called {
                cell.result = Some(Err("$DONE called more than once".to_string()));
            } else {
                cell.called = true;
                cell.result = Some(outcome);
            }
            Ok(JsValue::undefined())
        })
    };
    ctx.register_global_callable(js_string!("$DONE"), 1, done_fn)
        // UNWRAP-OK: fresh context cannot already define $DONE.
        .expect("$DONE: install on fresh context cannot fail");
}

/// Format a `$DONE(error)` argument into a failure message, preferring
/// `name: message` for error-like objects.
fn done_error_message(value: &JsValue, ctx: &mut Context) -> String {
    if let Some(object) = value.as_object() {
        let name = object
            .get(js_string!("name"), ctx)
            .ok()
            .filter(|v| !v.is_undefined())
            .and_then(|v| v.to_string(ctx).ok())
            .map(|s| s.to_std_string_lossy());
        let message = object
            .get(js_string!("message"), ctx)
            .ok()
            .filter(|v| !v.is_undefined())
            .and_then(|v| v.to_string(ctx).ok())
            .map(|s| s.to_std_string_lossy());
        if let (Some(name), Some(message)) = (&name, &message) {
            return format!("{name}: {message}");
        }
    }
    value.to_string(ctx).map_or_else(
        |_| "async test signaled failure".to_string(),
        |s| s.to_std_string_lossy(),
    )
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
                args.get(1),
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
                args.get(1),
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
                None,
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
    delay_ms: Option<&JsValue>,
) -> boa_engine::JsResult<JsValue> {
    let Some(callback) = callback.and_then(JsValue::as_object) else {
        return Ok(JsValue::from(0_u32));
    };
    if !callback.is_callable() {
        return Ok(JsValue::from(0_u32));
    }

    // HTML timers clamp negative and non-numeric delays to 0ms.
    let delay = delay_ms
        .and_then(JsValue::as_number)
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .map_or(Duration::ZERO, |ms| Duration::from_secs_f64(ms / 1000.0));
    let id = scheduler.borrow_mut().schedule(queue, delay);
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

fn install_websocket(ctx: &mut Context, sessions: &WsSessionsRef) {
    let sessions = Rc::clone(sessions);
    let next_key = Rc::new(RefCell::new(0_u64));
    // SAFETY: the capture is Rc-based session bookkeeping, no GC pointers.
    let constructor = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            websocket_session_constructor(&sessions, &next_key, args, ctx)
        })
    };
    let websocket = FunctionObjectBuilder::new(ctx.realm(), constructor)
        .name(js_string!("WebSocket"))
        .length(1)
        .constructor(true)
        .build();

    ctx.register_global_property(js_string!("WebSocket"), websocket, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("WebSocket: install on fresh context cannot fail");
}

/// A same-document navigation the page requested via history.pushState or
/// replaceState. The embedder drains these each tick and records them in its
/// session history without reloading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryIntent {
    pub replace: bool,
    pub url: String,
    /// JSON-serialized state (structured-clone-lite; documented limitation).
    pub state_json: String,
}

type HistoryIntentsRef = Rc<RefCell<Vec<HistoryIntent>>>;

type ViewportRef = Rc<std::cell::Cell<(f32, f32)>>;

/// Callback the embedder installs to serialize computed style values.
pub type ComputedStyleProvider = Rc<dyn Fn(silksurf_dom::NodeId, &str) -> Option<String>>;

/// `__silksurfMatchMedia(query)`: evaluate a media query prelude against the
/// current viewport through the silksurf-css evaluator.
fn install_match_media_native(ctx: &mut Context, viewport: &ViewportRef) {
    let viewport = Rc::clone(viewport);
    // SAFETY: the capture is Rc<Cell<(f32, f32)>>, no GC pointers.
    let native = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let query = args
                .first()
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let mut tokenizer = silksurf_css::CssTokenizer::new();
            let mut tokens = tokenizer.feed(query.as_str()).unwrap_or_default();
            tokens.extend(tokenizer.finish().unwrap_or_default());
            let (width, height) = viewport.get();
            let matches = silksurf_css::media::evaluate_media_query(&tokens, width, height);
            Ok(JsValue::from(matches))
        })
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfMatchMedia"), 1, native);
}

/// `__silksurfHistoryIntent(replace, url, stateJson)`: queue a same-document
/// navigation for the embedder to drain.
fn install_history_intent_native(ctx: &mut Context, intents: &HistoryIntentsRef) {
    let intents = Rc::clone(intents);
    // SAFETY: the capture is Rc<RefCell<Vec<HistoryIntent>>>, no GC pointers.
    let native = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let replace = args.first().is_some_and(boa_engine::JsValue::to_boolean);
            let url = args
                .get(1)
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_default();
            let state_json = args
                .get(2)
                .map(|value| value.to_string(ctx).map(|s| s.to_std_string_lossy()))
                .transpose()?
                .unwrap_or_else(|| "null".to_string());
            intents.borrow_mut().push(HistoryIntent {
                replace,
                url,
                state_json,
            });
            Ok(JsValue::undefined())
        })
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfHistoryIntent"), 3, native);
}

/// One live SSE subscription, keyed to its JS instance in the hidden
/// `__silksurfSseInstances` registry.
struct SseBinding {
    key: u64,
    subscription: silksurf_net::SseSubscription,
}

type SseSubscriptionsRef = Rc<RefCell<Vec<SseBinding>>>;

const SSE_INSTANCES_REGISTRY: &str = "__silksurfSseInstances";

fn sse_instance_object(key: u64, ctx: &mut Context) -> boa_engine::JsResult<Option<JsObject>> {
    let registry = event_dispatch::hidden_global_object(SSE_INSTANCES_REGISTRY, ctx)?;
    Ok(registry
        .get(JsString::from(key.to_string().as_str()), ctx)?
        .as_object())
}

fn sse_drop_instance(key: u64, ctx: &mut Context) -> boa_engine::JsResult<()> {
    let registry = event_dispatch::hidden_global_object(SSE_INSTANCES_REGISTRY, ctx)?;
    registry.set(
        JsString::from(key.to_string().as_str()),
        JsValue::undefined(),
        false,
        ctx,
    )?;
    Ok(())
}

/// `EventSource` constructor over `SseSubscription`. `readyState`: 0 CONNECTING,
/// 1 OPEN, 2 CLOSED (`EventSource` spec values). Events arrive through the
/// host-callback drain; named events dispatch to `on<type>` handlers with
/// `onmessage` as the default.
fn install_event_source(ctx: &mut Context, subscriptions: &SseSubscriptionsRef) {
    let subscriptions = Rc::clone(subscriptions);
    let next_key = Rc::new(RefCell::new(0_u64));
    // SAFETY: the capture is Rc-based subscription bookkeeping, no GC pointers.
    let constructor = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(url_value) = args.first() else {
                return Err(JsNativeError::typ()
                    .with_message("EventSource: URL argument is required")
                    .into());
            };
            let url = url_value.to_string(ctx)?.to_std_string_lossy();
            let key = {
                let mut next = next_key.borrow_mut();
                let key = *next;
                *next += 1;
                key
            };
            let subscription = silksurf_net::SseSubscription::connect(url.as_str());
            subscriptions
                .borrow_mut()
                .push(SseBinding { key, subscription });

            let close_subscriptions = Rc::clone(&subscriptions);
            // SAFETY (inherited from the enclosing unsafe block): Rc-based
            // bookkeeping capture, no GC pointers.
            let close_fn = {
                NativeFunction::from_closure(move |this, _args, ctx| {
                    // Dropping the subscription severs the channel; the
                    // reader thread exits on its next send.
                    close_subscriptions
                        .borrow_mut()
                        .retain(|binding| binding.key != key);
                    if let Some(object) = this.as_object() {
                        object.set(js_string!("readyState"), 2_u32, false, ctx)?;
                    }
                    Ok(JsValue::undefined())
                })
            };

            let source = ObjectInitializer::new(ctx)
                .property(
                    js_string!("url"),
                    JsString::from(url.as_str()),
                    Attribute::all(),
                )
                .property(js_string!("readyState"), 0_u32, Attribute::all())
                .property(js_string!("onopen"), JsValue::null(), Attribute::all())
                .property(js_string!("onmessage"), JsValue::null(), Attribute::all())
                .property(js_string!("onerror"), JsValue::null(), Attribute::all())
                .function(close_fn, js_string!("close"), 0)
                .build();
            let registry = event_dispatch::hidden_global_object(SSE_INSTANCES_REGISTRY, ctx)?;
            registry.set(
                JsString::from(key.to_string().as_str()),
                source.clone(),
                false,
                ctx,
            )?;
            Ok(JsValue::from(source))
        })
    };
    let event_source = FunctionObjectBuilder::new(ctx.realm(), constructor)
        .name(js_string!("EventSource"))
        .length(1)
        .constructor(true)
        .build();
    ctx.register_global_property(js_string!("EventSource"), event_source, Attribute::all())
        // UNWRAP-OK: The preceding initialization operation is invariant for this construction path.

        .expect("EventSource: install on fresh context cannot fail");
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

/// One live WebSocket transport, keyed to its JS instance in the hidden
/// `__silksurfWsInstances` registry (GC-rooted there, never in Rust).
struct WsBinding {
    key: u64,
    session: silksurf_net::WebSocketSession,
}

type WsSessionsRef = Rc<RefCell<Vec<WsBinding>>>;

const WS_INSTANCES_REGISTRY: &str = "__silksurfWsInstances";

fn ws_instance_object(key: u64, ctx: &mut Context) -> boa_engine::JsResult<Option<JsObject>> {
    let registry = event_dispatch::hidden_global_object(WS_INSTANCES_REGISTRY, ctx)?;
    Ok(registry
        .get(JsString::from(key.to_string().as_str()), ctx)?
        .as_object())
}

fn ws_drop_instance(key: u64, ctx: &mut Context) -> boa_engine::JsResult<()> {
    let registry = event_dispatch::hidden_global_object(WS_INSTANCES_REGISTRY, ctx)?;
    registry.set(
        JsString::from(key.to_string().as_str()),
        JsValue::undefined(),
        false,
        ctx,
    )?;
    Ok(())
}

/// Session-backed WebSocket constructor: connects in the background and
/// delivers open/message/error/close through the host-callback drain.
fn websocket_session_constructor(
    sessions: &WsSessionsRef,
    next_key: &Rc<RefCell<u64>>,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(url_value) = args.first() else {
        return Err(JsNativeError::typ()
            .with_message("WebSocket: URL argument is required")
            .into());
    };
    let url = url_value.to_string(ctx)?.to_std_string_lossy();

    let key = {
        let mut next = next_key.borrow_mut();
        let key = *next;
        *next += 1;
        key
    };
    let session = silksurf_net::WebSocketSession::connect(url.as_str());
    sessions.borrow_mut().push(WsBinding { key, session });

    let send_sessions = Rc::clone(sessions);
    // SAFETY: the capture is Rc<RefCell<Vec<WsBinding>>> + u64, no GC pointers.
    let send_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let payload = args
                .first()
                .map(|value| value.to_string(ctx))
                .transpose()?
                .map(|value| value.to_std_string_lossy())
                .unwrap_or_default();
            let sessions = send_sessions.borrow();
            if let Some(binding) = sessions.iter().find(|binding| binding.key == key) {
                // Frames queued before the handshake finishes are buffered by
                // the session and flushed once the socket opens.
                let _ = binding.session.send_text(payload);
            }
            Ok(JsValue::undefined())
        })
    };

    let close_sessions = Rc::clone(sessions);
    // SAFETY: the capture is Rc<RefCell<Vec<WsBinding>>> + u64, no GC pointers.
    let close_fn = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            let sessions = close_sessions.borrow();
            if let Some(binding) = sessions.iter().find(|binding| binding.key == key) {
                binding.session.close();
            }
            Ok(JsValue::undefined())
        })
    };

    let socket = ObjectInitializer::new(ctx)
        .property(
            js_string!("url"),
            JsString::from(url.as_str()),
            Attribute::all(),
        )
        .property(js_string!("readyState"), 0_u32, Attribute::all())
        .property(js_string!("lastMessage"), js_string!(""), Attribute::all())
        .property(js_string!("lastError"), js_string!(""), Attribute::all())
        .property(js_string!("onopen"), JsValue::null(), Attribute::all())
        .property(js_string!("onmessage"), JsValue::null(), Attribute::all())
        .property(js_string!("onerror"), JsValue::null(), Attribute::all())
        .property(js_string!("onclose"), JsValue::null(), Attribute::all())
        .function(send_fn, js_string!("send"), 1)
        .function(close_fn, js_string!("close"), 0)
        .build();

    let registry = event_dispatch::hidden_global_object(WS_INSTANCES_REGISTRY, ctx)?;
    registry.set(
        JsString::from(key.to_string().as_str()),
        socket.clone(),
        false,
        ctx,
    )?;
    Ok(JsValue::from(socket))
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

/// `performance.now()`: fractional milliseconds since a process-wide epoch.
///
/// The epoch is the first call's instant, so the first reading is ~0 and every
/// later reading is a monotonic elapsed time suitable for benchmark deltas.
fn performance_now(
    _this: &JsValue,
    _args: &[JsValue],
    _ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    Ok(JsValue::from(epoch.elapsed().as_secs_f64() * 1000.0))
}

// ---- XMLHttpRequest implementation -----------------------------------------

// readyState values from the XHR specification.
const XHR_UNSENT: u8 = 0;
const XHR_OPENED: u8 = 1;
const XHR_HEADERS_RECEIVED: u8 = 2;
const XHR_LOADING: u8 = 3;
const XHR_DONE: u8 = 4;

/// Install `XMLHttpRequest` as a global constructor.
///
/// The request executes synchronously inside `send()` (the browser's blocking
/// net path), so the readyState progression and the load/readystatechange
/// events all fire before `send()` returns. This matches synchronous XHR and
/// serves benchmark harnesses that use XHR only to pull resource files.
fn install_xml_http_request(ctx: &mut Context) {
    let constructor =
        FunctionObjectBuilder::new(ctx.realm(), NativeFunction::from_fn_ptr(xhr_constructor))
            .name(js_string!("XMLHttpRequest"))
            .length(0)
            .constructor(true)
            .build();
    let constructor_object: JsObject = constructor.clone().into();
    for (name, value) in [
        ("UNSENT", XHR_UNSENT),
        ("OPENED", XHR_OPENED),
        ("HEADERS_RECEIVED", XHR_HEADERS_RECEIVED),
        ("LOADING", XHR_LOADING),
        ("DONE", XHR_DONE),
    ] {
        constructor_object
            .set(js_string!(name), value, false, ctx)
            // UNWRAP-OK: fresh constructor cannot already carry the constant.
            .expect("XMLHttpRequest readyState constant install cannot fail");
    }
    ctx.register_global_property(js_string!("XMLHttpRequest"), constructor, Attribute::all())
        // UNWRAP-OK: fresh context cannot already define XMLHttpRequest.
        .expect("XMLHttpRequest: install on fresh context cannot fail");
}

fn xhr_constructor(
    _this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let request_headers = JsArray::new(ctx);
    let response_headers = JsArray::new(ctx);
    let load_listeners = JsArray::new(ctx);
    let error_listeners = JsArray::new(ctx);
    let readystate_listeners = JsArray::new(ctx);
    let instance = ObjectInitializer::new(ctx)
        .property(js_string!("readyState"), XHR_UNSENT, Attribute::all())
        .property(js_string!("status"), 0_u32, Attribute::all())
        .property(js_string!("statusText"), js_string!(""), Attribute::all())
        .property(js_string!("responseText"), js_string!(""), Attribute::all())
        .property(js_string!("response"), js_string!(""), Attribute::all())
        .property(js_string!("responseType"), js_string!(""), Attribute::all())
        .property(
            js_string!("onreadystatechange"),
            JsValue::null(),
            Attribute::all(),
        )
        .property(js_string!("onload"), JsValue::null(), Attribute::all())
        .property(js_string!("onerror"), JsValue::null(), Attribute::all())
        .property(
            js_string!("__xhrMethod"),
            js_string!("GET"),
            Attribute::all(),
        )
        .property(js_string!("__xhrUrl"), js_string!(""), Attribute::all())
        .property(
            js_string!("__xhrRequestHeaders"),
            request_headers,
            Attribute::all(),
        )
        .property(
            js_string!("__xhrResponseHeaders"),
            response_headers,
            Attribute::all(),
        )
        .property(
            js_string!("__xhrLoadListeners"),
            load_listeners,
            Attribute::all(),
        )
        .property(
            js_string!("__xhrErrorListeners"),
            error_listeners,
            Attribute::all(),
        )
        .property(
            js_string!("__xhrReadyStateListeners"),
            readystate_listeners,
            Attribute::all(),
        )
        .function(NativeFunction::from_fn_ptr(xhr_open), js_string!("open"), 2)
        .function(
            NativeFunction::from_fn_ptr(xhr_set_request_header),
            js_string!("setRequestHeader"),
            2,
        )
        .function(NativeFunction::from_fn_ptr(xhr_send), js_string!("send"), 1)
        .function(
            NativeFunction::from_fn_ptr(xhr_abort),
            js_string!("abort"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(xhr_get_all_response_headers),
            js_string!("getAllResponseHeaders"),
            0,
        )
        .function(
            NativeFunction::from_fn_ptr(xhr_get_response_header),
            js_string!("getResponseHeader"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(xhr_add_event_listener),
            js_string!("addEventListener"),
            2,
        )
        .build();
    Ok(instance.into())
}

fn xhr_open(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    let Some(instance) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    let method = args
        .first()
        .map(|value| value.to_string(ctx))
        .transpose()?
        .map_or_else(|| "GET".to_string(), |s| s.to_std_string_lossy());
    let url = args
        .get(1)
        .map(|value| value.to_string(ctx))
        .transpose()?
        .map_or_else(String::new, |s| s.to_std_string_lossy());
    instance.set(
        js_string!("__xhrMethod"),
        js_string!(method.as_str()),
        false,
        ctx,
    )?;
    instance.set(js_string!("__xhrUrl"), js_string!(url.as_str()), false, ctx)?;
    // A fresh open() resets any accumulated request headers and prior result.
    instance.set(
        js_string!("__xhrRequestHeaders"),
        JsArray::new(ctx),
        false,
        ctx,
    )?;
    instance.set(js_string!("status"), 0_u32, false, ctx)?;
    instance.set(js_string!("responseText"), js_string!(""), false, ctx)?;
    set_xhr_ready_state(&instance, XHR_OPENED, ctx)?;
    Ok(JsValue::undefined())
}

fn xhr_set_request_header(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(instance) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    let (Some(name), Some(value)) = (args.first(), args.get(1)) else {
        return Ok(JsValue::undefined());
    };
    let name = name.to_string(ctx)?;
    let value = value.to_string(ctx)?;
    let headers = instance.get(js_string!("__xhrRequestHeaders"), ctx)?;
    if let Some(headers) = headers.as_object() {
        let headers = JsArray::from_object(headers)?;
        let pair = JsArray::new(ctx);
        pair.push(JsValue::from(name), ctx)?;
        pair.push(JsValue::from(value), ctx)?;
        headers.push(JsValue::from(pair), ctx)?;
    }
    Ok(JsValue::undefined())
}

fn xhr_send(this: &JsValue, args: &[JsValue], ctx: &mut Context) -> boa_engine::JsResult<JsValue> {
    use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};

    let Some(instance) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    let method_str = instance
        .get(js_string!("__xhrMethod"), ctx)?
        .to_string(ctx)?
        .to_std_string_lossy();
    let url = instance
        .get(js_string!("__xhrUrl"), ctx)?
        .to_string(ctx)?
        .to_std_string_lossy();
    let method = match method_str.to_ascii_uppercase().as_str() {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "DELETE" => HttpMethod::Delete,
        _ => HttpMethod::Get,
    };
    let body = match args.first() {
        Some(value) if !value.is_undefined() && !value.is_null() => {
            value.to_string(ctx)?.to_std_string_lossy().into_bytes()
        }
        _ => Vec::new(),
    };
    let headers = collect_xhr_request_headers(&instance, ctx)?;

    let request = HttpRequest {
        method,
        url,
        headers,
        body,
    };

    match BasicClient::new().fetch(&request) {
        Ok(response) => {
            store_xhr_response(&instance, &response, ctx)?;
            set_xhr_ready_state(&instance, XHR_HEADERS_RECEIVED, ctx)?;
            set_xhr_ready_state(&instance, XHR_LOADING, ctx)?;
            set_xhr_ready_state(&instance, XHR_DONE, ctx)?;
            fire_xhr_event(&instance, "load", "__xhrLoadListeners", "onload", ctx)?;
        }
        Err(err) => {
            instance.set(js_string!("status"), 0_u32, false, ctx)?;
            instance.set(
                js_string!("statusText"),
                js_string!(err.message.as_str()),
                false,
                ctx,
            )?;
            set_xhr_ready_state(&instance, XHR_DONE, ctx)?;
            fire_xhr_event(&instance, "error", "__xhrErrorListeners", "onerror", ctx)?;
        }
    }
    Ok(JsValue::undefined())
}

fn xhr_abort(
    _this: &JsValue,
    _args: &[JsValue],
    _ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    // The request completes synchronously inside send(); there is no in-flight
    // transfer to cancel, so abort() is a no-op after the fact.
    Ok(JsValue::undefined())
}

fn xhr_get_all_response_headers(
    this: &JsValue,
    _args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(instance) = this.as_object() else {
        return Ok(JsValue::from(js_string!("")));
    };
    let headers = instance.get(js_string!("__xhrResponseHeaders"), ctx)?;
    let mut out = String::new();
    if let Some(headers) = headers.as_object() {
        let headers = JsArray::from_object(headers)?;
        let length = headers.length(ctx)?;
        for index in 0..length {
            let pair = headers.get(index, ctx)?;
            if let Some(pair) = pair.as_object() {
                let pair = JsArray::from_object(pair)?;
                let name = pair.get(0_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
                let value = pair.get(1_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
                let _ = write!(out, "{name}: {value}\r\n");
            }
        }
    }
    Ok(JsValue::from(js_string!(out.as_str())))
}

fn xhr_get_response_header(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(instance) = this.as_object() else {
        return Ok(JsValue::null());
    };
    let Some(target) = args.first() else {
        return Ok(JsValue::null());
    };
    let target = target.to_string(ctx)?.to_std_string_lossy();
    let headers = instance.get(js_string!("__xhrResponseHeaders"), ctx)?;
    if let Some(headers) = headers.as_object() {
        let headers = JsArray::from_object(headers)?;
        let length = headers.length(ctx)?;
        for index in 0..length {
            let pair = headers.get(index, ctx)?;
            if let Some(pair) = pair.as_object() {
                let pair = JsArray::from_object(pair)?;
                let name = pair.get(0_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
                if name.eq_ignore_ascii_case(&target) {
                    return pair.get(1_u64, ctx);
                }
            }
        }
    }
    Ok(JsValue::null())
}

fn xhr_add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    ctx: &mut Context,
) -> boa_engine::JsResult<JsValue> {
    let Some(instance) = this.as_object() else {
        return Ok(JsValue::undefined());
    };
    let (Some(kind), Some(listener)) = (args.first(), args.get(1)) else {
        return Ok(JsValue::undefined());
    };
    let Some(listener) = listener.as_object().filter(JsObject::is_callable) else {
        return Ok(JsValue::undefined());
    };
    let kind = kind.to_string(ctx)?.to_std_string_lossy();
    let slot = match kind.as_str() {
        "load" => "__xhrLoadListeners",
        "error" => "__xhrErrorListeners",
        "readystatechange" => "__xhrReadyStateListeners",
        _ => return Ok(JsValue::undefined()),
    };
    let listeners = instance.get(js_string!(slot), ctx)?;
    if let Some(listeners) = listeners.as_object() {
        JsArray::from_object(listeners)?.push(JsValue::from(listener.clone()), ctx)?;
    }
    Ok(JsValue::undefined())
}

/// Advance readyState and fire the readystatechange handler plus listeners.
fn set_xhr_ready_state(
    instance: &JsObject,
    state: u8,
    ctx: &mut Context,
) -> boa_engine::JsResult<()> {
    instance.set(js_string!("readyState"), state, false, ctx)?;
    fire_xhr_event(
        instance,
        "readystatechange",
        "__xhrReadyStateListeners",
        "onreadystatechange",
        ctx,
    )
}

/// Invoke the on-property handler and every registered listener for an event.
fn fire_xhr_event(
    instance: &JsObject,
    _event_name: &str,
    listener_slot: &str,
    property: &str,
    ctx: &mut Context,
) -> boa_engine::JsResult<()> {
    call_optional_method(instance, js_string!(property), &[], ctx)?;
    let listeners = instance.get(js_string!(listener_slot), ctx)?;
    if let Some(listeners) = listeners.as_object() {
        let listeners = JsArray::from_object(listeners)?;
        let length = listeners.length(ctx)?;
        for index in 0..length {
            let listener = listeners.get(index, ctx)?;
            if let Some(listener) = listener.as_object().filter(JsObject::is_callable) {
                listener.call(&JsValue::from(instance.clone()), &[], ctx)?;
            }
        }
    }
    Ok(())
}

fn collect_xhr_request_headers(
    instance: &JsObject,
    ctx: &mut Context,
) -> boa_engine::JsResult<Vec<(String, String)>> {
    let mut out = Vec::new();
    let headers = instance.get(js_string!("__xhrRequestHeaders"), ctx)?;
    if let Some(headers) = headers.as_object() {
        let headers = JsArray::from_object(headers)?;
        let length = headers.length(ctx)?;
        for index in 0..length {
            let pair = headers.get(index, ctx)?;
            if let Some(pair) = pair.as_object() {
                let pair = JsArray::from_object(pair)?;
                let name = pair.get(0_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
                let value = pair.get(1_u64, ctx)?.to_string(ctx)?.to_std_string_lossy();
                out.push((name, value));
            }
        }
    }
    if out
        .iter()
        .all(|(name, _)| !name.eq_ignore_ascii_case("Accept"))
    {
        out.push(("Accept".to_string(), "*/*".to_string()));
    }
    Ok(out)
}

fn store_xhr_response(
    instance: &JsObject,
    response: &silksurf_net::HttpResponse,
    ctx: &mut Context,
) -> boa_engine::JsResult<()> {
    let body = String::from_utf8_lossy(&response.body).to_string();
    instance.set(js_string!("status"), u32::from(response.status), false, ctx)?;
    instance.set(
        js_string!("statusText"),
        js_string!(http_status_text(response.status)),
        false,
        ctx,
    )?;
    instance.set(
        js_string!("responseText"),
        js_string!(body.as_str()),
        false,
        ctx,
    )?;
    instance.set(
        js_string!("response"),
        js_string!(body.as_str()),
        false,
        ctx,
    )?;
    let response_headers = JsArray::new(ctx);
    for (name, value) in &response.headers {
        let pair = JsArray::new(ctx);
        pair.push(JsValue::from(js_string!(name.as_str())), ctx)?;
        pair.push(JsValue::from(js_string!(value.as_str())), ctx)?;
        response_headers.push(JsValue::from(pair), ctx)?;
    }
    instance.set(
        js_string!("__xhrResponseHeaders"),
        response_headers,
        false,
        ctx,
    )?;
    Ok(())
}

fn http_status_text(status: u16) -> &'static str {
    match status {
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
    }
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

const PENDING_NET_REGISTRY: &str = "__silksurfPendingNet";

/// Build the `HttpRequest` a `fetch()` call describes: method/headers/body from
/// the init object (second argument), GET with Accept: */* by default.
fn fetch_request_from_init(
    url: String,
    init: Option<&JsValue>,
    ctx: &mut Context,
) -> boa_engine::JsResult<silksurf_net::HttpRequest> {
    use silksurf_net::{HttpMethod, HttpRequest};
    let mut method = HttpMethod::Get;
    let mut headers: Vec<(String, String)> = vec![("Accept".to_owned(), "*/*".to_owned())];
    let mut body = Vec::new();

    if let Some(init) = init.and_then(JsValue::as_object) {
        let method_value = init.get(js_string!("method"), ctx)?;
        if !method_value.is_undefined() {
            let name = method_value.to_string(ctx)?.to_std_string_lossy();
            method = match name.to_ascii_uppercase().as_str() {
                "POST" => HttpMethod::Post,
                _ => HttpMethod::Get,
            };
        }
        let headers_value = init.get(js_string!("headers"), ctx)?;
        if let Some(headers_object) = headers_value.as_object() {
            for key in headers_object.own_property_keys(ctx)? {
                let name = key.to_string();
                let value = headers_object
                    .get(key, ctx)?
                    .to_string(ctx)?
                    .to_std_string_lossy();
                headers.push((name, value));
            }
        }
        let body_value = init.get(js_string!("body"), ctx)?;
        if !body_value.is_undefined() && !body_value.is_null() {
            body = body_value
                .to_string(ctx)?
                .to_std_string_lossy()
                .into_bytes();
        }
    }

    Ok(HttpRequest {
        method,
        url,
        headers,
        body,
    })
}

/// Park a pending promise's resolving functions under the request id.
/// JS-side storage keeps them GC-rooted until the completion drains.
fn park_net_resolvers(
    id: u64,
    functions: &boa_engine::builtins::promise::ResolvingFunctions,
    ctx: &mut Context,
) -> boa_engine::JsResult<()> {
    let registry = event_dispatch::hidden_global_object(PENDING_NET_REGISTRY, ctx)?;
    let entry = ObjectInitializer::new(ctx)
        .property(
            js_string!("resolve"),
            functions.resolve.clone(),
            Attribute::all(),
        )
        .property(
            js_string!("reject"),
            functions.reject.clone(),
            Attribute::all(),
        )
        .build();
    registry.set(JsString::from(id.to_string().as_str()), entry, false, ctx)?;
    Ok(())
}

fn take_net_resolvers(
    id: u64,
    ctx: &mut Context,
) -> boa_engine::JsResult<Option<(JsValue, JsValue)>> {
    let registry = event_dispatch::hidden_global_object(PENDING_NET_REGISTRY, ctx)?;
    let key = JsString::from(id.to_string().as_str());
    let entry = registry.get(key.clone(), ctx)?;
    let Some(entry) = entry.as_object() else {
        return Ok(None);
    };
    let resolve = entry.get(js_string!("resolve"), ctx)?;
    let reject = entry.get(js_string!("reject"), ctx)?;
    registry.set(key, JsValue::undefined(), false, ctx)?;
    Ok(Some((resolve, reject)))
}

/// Register the queue-backed `fetch()` global.
fn install_async_fetch(ctx: &mut Context, shared: &net_queue::NetSharedRef) {
    let shared = Rc::clone(shared);
    // SAFETY: the capture is Rc<RefCell<NetShared>>, which holds no boa
    // GC-managed pointers; Boa stores the closure for the function lifetime.
    let fetch_fn = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let Some(input) = args.first() else {
                let err = JsNativeError::typ().with_message("fetch: URL argument is required");
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
            let request = fetch_request_from_init(url, args.get(1), ctx)?;
            let (promise, functions) = JsPromise::new_pending(ctx);
            let (id, tx) = shared.borrow_mut().begin_request();
            park_net_resolvers(id, &functions, ctx)?;
            net_queue::spawn_request(id, tx, request);
            Ok(JsValue::from(promise))
        })
    };
    // UNWRAP-OK: fresh Context cannot already have "fetch" defined.
    ctx.register_global_callable(js_string!("fetch"), 1, fetch_fn)
        .expect("fetch: install on fresh context cannot fail");
}

/// Build a plain JS Response-like object from an HTTP response.
///
/// Exposes: status (u32), ok (bool), statusText (string),
/// `text()` -> `Promise<string>`, `json()` -> `Promise<object>`.
/// Bytes handed to `reader.read()` per resolution. The body is already fully
/// buffered (socket-level streaming is a named follow-up inside `BasicClient`);
/// slicing keeps consumer loops (`while (!done) read()`) exercised the way a
/// streamed response will exercise them.
const RESPONSE_BODY_CHUNK_BYTES: usize = 16 * 1024;

/// Build `response.body`: an object whose `getReader().read()` drains the
/// buffered body chunk by chunk as `{value: Uint8Array, done}` promises.
fn build_response_body_stream(body: &[u8], ctx: &mut Context) -> JsValue {
    use boa_engine::object::builtins::JsUint8Array;

    let chunks: std::collections::VecDeque<Vec<u8>> = body
        .chunks(RESPONSE_BODY_CHUNK_BYTES)
        .map(<[u8]>::to_vec)
        .collect();
    let store = Rc::new(RefCell::new(chunks));

    // SAFETY: the capture is Rc<RefCell<VecDeque<Vec<u8>>>>, no GC pointers.
    let get_reader = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let store = Rc::clone(&store);
            // SAFETY (inherited from the enclosing unsafe block): same
            // capture shape; per-reader clone of the store.
            let read_fn = {
                NativeFunction::from_closure(move |_this, _args, ctx| {
                    let next = store.borrow_mut().pop_front();
                    let result = match next {
                        Some(chunk) => {
                            let value = JsUint8Array::from_iter(chunk, ctx)?;
                            ObjectInitializer::new(ctx)
                                .property(js_string!("value"), value, Attribute::all())
                                .property(js_string!("done"), false, Attribute::all())
                                .build()
                        }
                        None => ObjectInitializer::new(ctx)
                            .property(js_string!("value"), JsValue::undefined(), Attribute::all())
                            .property(js_string!("done"), true, Attribute::all())
                            .build(),
                    };
                    Ok(JsValue::from(JsPromise::from_result::<
                        JsValue,
                        JsNativeError,
                    >(
                        Ok(JsValue::from(result)), ctx
                    )))
                })
            };
            let cancel_fn = NativeFunction::from_fn_ptr(|_this, _args, ctx| {
                Ok(JsValue::from(JsPromise::from_result::<
                    JsValue,
                    JsNativeError,
                >(
                    Ok(JsValue::undefined()), ctx
                )))
            });
            let release_fn = NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined()));
            let reader = ObjectInitializer::new(ctx)
                .function(read_fn, js_string!("read"), 0)
                .function(cancel_fn, js_string!("cancel"), 0)
                .function(release_fn, js_string!("releaseLock"), 0)
                .build();
            Ok(JsValue::from(reader))
        })
    };

    ObjectInitializer::new(ctx)
        .function(get_reader, js_string!("getReader"), 0)
        .property(js_string!("locked"), false, Attribute::all())
        .build()
        .into()
}

fn build_response_object(response: silksurf_net::HttpResponse, ctx: &mut Context) -> JsValue {
    let status = response.status;
    let body = response.body;

    let status_text = http_status_text(status);
    let body_stream = build_response_body_stream(&body, ctx);

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
        .property(js_string!("body"), body_stream, Attribute::all())
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
    fn document_cookie_rejects_http_only_assignment() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "document.cookie = 'visible=1'; \
             document.cookie = 'secret=2; HttpOnly'; \
             globalThis.jar = document.cookie;",
        )
        .expect("script sets cookies");
        // A script cannot set an HttpOnly cookie, so it never appears.
        let jar = global_string(&mut ctx, "jar");
        assert!(jar.contains("visible=1"), "jar: {jar}");
        assert!(!jar.contains("secret"), "HttpOnly cookie leaked: {jar}");
    }

    #[test]
    fn shared_jar_bridges_document_cookie_and_http() {
        // A partitioned jar shared with the HTTP client: an HTTP-set cookie in
        // the top-level document's first-party partition is readable via
        // document.cookie, and a document.cookie write lands in the same
        // partition the first-party HTTP client would read/send.
        use silksurf_net::cookie::{PartitionedCookieStore, partition_key};
        let top = "https://example.com";
        let dom = std::sync::Arc::new(std::sync::Mutex::new(silksurf_dom::Dom::new()));
        let jar = std::sync::Arc::new(std::sync::Mutex::new(PartitionedCookieStore::new()));
        // First-party partition = partition_key(top, top).
        jar.lock()
            .unwrap()
            .store_mut(&partition_key(top, top))
            .set_from_set_cookie("sid=fromhttp", "example.com", 0);

        let mut ctx = SilkContext::with_dom_and_cookies(&dom, &jar, top, "example.com");
        ctx.eval("globalThis.seen = document.cookie; document.cookie = 'pref=dark';")
            .expect("script reads and writes document.cookie");

        assert_eq!(global_string(&mut ctx, "seen"), "sid=fromhttp");
        // The document.cookie write is visible in the same first-party partition.
        let jar = jar.lock().unwrap();
        let store = jar
            .store(&partition_key(top, top))
            .expect("first-party partition exists");
        let header = store.cookie_header(
            "example.com",
            "/",
            true,
            true,
            silksurf_net::cookie::SameSiteContext::Unknown,
            0,
        );
        assert!(header.contains("sid=fromhttp"), "header: {header}");
        assert!(header.contains("pref=dark"), "header: {header}");
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

        // Intervals are deadline-ordered: each firing becomes due only after
        // its 1ms period elapses, so the drain waits out the period first.
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 1);
        assert_number_eq(&mut ctx, "count", 1.0);
        assert!(ctx.has_pending_host_callbacks());
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert_eq!(ctx.run_ready_host_callbacks().unwrap(), 1);
        assert_number_eq(&mut ctx, "count", 2.0);
        assert!(!ctx.has_pending_host_callbacks());
    }

    #[test]
    fn websocket_session_delivers_open_message_and_close() {
        let (url, server) = start_websocket_echo_server();
        let mut ctx = SilkContext::new();
        ctx.eval(
            format!(
                "globalThis.wsEvents = []; \
                 globalThis.wsData = ''; \
                 var ws = new WebSocket('{url}'); \
                 ws.onopen = function () {{ globalThis.wsEvents.push('open:' + ws.readyState); }}; \
                 ws.onmessage = function (event) {{ \
                   globalThis.wsEvents.push('message'); \
                   globalThis.wsData = event.data; \
                 }}; \
                 ws.onclose = function () {{ globalThis.wsEvents.push('close:' + ws.readyState); }}; \
                 ws.send('hello-ai');"
            )
            .as_str(),
        )
        .expect("script opens websocket");

        // Pump until the close handler fires; the session delivers open,
        // the echoed frame, then close, all through host-callback drains.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let _ = ctx.run_ready_host_callbacks();
            let events = global_string(&mut ctx, "wsEvents");
            if events.contains("close") || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        server.join().expect("echo server exits");
        assert_eq!(
            global_string(&mut ctx, "wsEvents"),
            "open:1,message,close:3"
        );
        assert_eq!(global_string(&mut ctx, "wsData"), "hello-ai");
    }

    #[test]
    fn websocket_connect_failure_fires_onerror_then_onclose() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.wsErrorHit = false; \
             globalThis.wsErrorMessage = ''; \
             globalThis.wsClosed = false; \
             var ws = new WebSocket('ws://127.0.0.1:1'); \
             ws.onerror = function (event) { \
               globalThis.wsErrorHit = true; \
               globalThis.wsErrorMessage = event.message; \
             }; \
             ws.onclose = function () { globalThis.wsClosed = true; };",
        )
        .expect("script constructs websocket");

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let _ = ctx.run_ready_host_callbacks();
            if global_bool(&mut ctx, "wsClosed") || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(global_bool(&mut ctx, "wsErrorHit"));
        assert!(!global_string(&mut ctx, "wsErrorMessage").is_empty());
        assert!(global_bool(&mut ctx, "wsClosed"));
    }

    #[test]
    fn queue_microtask_runs_before_timers() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "globalThis.order = []; \
             setTimeout(function () { globalThis.order.push('timer'); }, 0); \
             queueMicrotask(function () { globalThis.order.push('micro'); });",
        )
        .expect("script schedules work");
        ctx.run_pending_jobs();
        let _ = ctx.run_ready_host_callbacks();
        assert_eq!(global_string(&mut ctx, "order"), "micro,timer");
    }

    #[test]
    fn match_media_evaluates_against_set_viewport() {
        let mut ctx = SilkContext::new();
        ctx.set_viewport(800.0, 600.0);
        ctx.eval(
            "globalThis.wide = matchMedia('(min-width: 700px)').matches; \
             globalThis.narrow = matchMedia('(min-width: 900px)').matches;",
        )
        .expect("script evaluates media queries");
        assert!(global_bool(&mut ctx, "wide"));
        assert!(!global_bool(&mut ctx, "narrow"));
        ctx.set_viewport(1000.0, 600.0);
        ctx.eval("globalThis.nowWide = matchMedia('(min-width: 900px)').matches;")
            .expect("script re-evaluates");
        assert!(global_bool(&mut ctx, "nowWide"));
    }

    #[test]
    fn push_state_queues_history_intents_and_updates_location() {
        let mut ctx = SilkContext::new();
        ctx.eval(
            "history.pushState({page: 1}, '', '/one'); \
             history.replaceState({page: 2}, '', '/two'); \
             globalThis.statePage = history.state.page; \
             globalThis.loc = location.href;",
        )
        .expect("script drives history");
        assert_number_eq(&mut ctx, "statePage", 2.0);
        assert_eq!(global_string(&mut ctx, "loc"), "/two");
        let intents = ctx.take_history_intents();
        assert_eq!(intents.len(), 2);
        assert!(!intents[0].replace);
        assert_eq!(intents[0].url, "/one");
        assert_eq!(intents[0].state_json, "{\"page\":1}");
        assert!(intents[1].replace);
        assert!(ctx.take_history_intents().is_empty());
    }

    #[test]
    fn local_storage_preload_and_dirty_snapshot_roundtrip() {
        let mut ctx = SilkContext::new();
        let mut seed = HashMap::new();
        seed.insert("token".to_string(), "abc".to_string());
        ctx.preload_local_storage(seed);
        assert!(ctx.take_local_storage_if_dirty().is_none());
        ctx.eval(
            "if (localStorage.getItem('token') !== 'abc') { throw new Error('preload'); } \
             localStorage.setItem('theme', 'dark');",
        )
        .expect("script reads preload and writes");
        let snapshot = ctx
            .take_local_storage_if_dirty()
            .expect("write marks dirty");
        assert_eq!(snapshot.get("theme").map(String::as_str), Some("dark"));
        assert_eq!(snapshot.get("token").map(String::as_str), Some("abc"));
        assert!(ctx.take_local_storage_if_dirty().is_none());
    }

    #[test]
    fn computed_style_provider_backs_get_computed_style() {
        let mut dom = silksurf_dom::Dom::new();
        let document = dom.create_document();
        let probe = dom.create_element("div");
        dom.set_attribute(probe, "id", "probe").expect("id sets");
        dom.append_child(document, probe).expect("probe attaches");
        dom.materialize_resolve_table();
        let arc = Arc::new(Mutex::new(dom));
        let mut ctx = SilkContext::with_dom(&arc);
        let target = probe;
        ctx.set_computed_style_provider(Rc::new(move |queried, prop| {
            (queried == target && prop == "background-color").then(|| "rgb(255, 0, 0)".to_string())
        }));
        ctx.eval(
            "var el = document.getElementById('probe'); \
             globalThis.bg = getComputedStyle(el).backgroundColor; \
             globalThis.viaFn = getComputedStyle(el).getPropertyValue('background-color'); \
             globalThis.missing = getComputedStyle(el).width;",
        )
        .expect("script reads computed style");
        assert_eq!(global_string(&mut ctx, "bg"), "rgb(255, 0, 0)");
        assert_eq!(global_string(&mut ctx, "viaFn"), "rgb(255, 0, 0)");
        assert_eq!(global_string(&mut ctx, "missing"), "");
    }

    #[test]
    fn event_source_streams_named_and_default_events() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("sse server binds");
        let addr = listener.local_addr().expect("sse server has addr");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client connects");
            let mut discard = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut discard);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                      data: tick\n\nevent: delta\ndata: tock\n\n",
                )
                .expect("server writes stream");
        });

        let mut ctx = SilkContext::new();
        ctx.eval(
            format!(
                "globalThis.sseLog = []; \
                 var es = new EventSource('http://{addr}/stream'); \
                 es.onopen = function () {{ globalThis.sseLog.push('open:' + es.readyState); }}; \
                 es.onmessage = function (e) {{ globalThis.sseLog.push('msg:' + e.data); }}; \
                 es.ondelta = function (e) {{ globalThis.sseLog.push('delta:' + e.data); }};"
            )
            .as_str(),
        )
        .expect("script constructs EventSource");

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let _ = ctx.run_ready_host_callbacks();
            let log = global_string(&mut ctx, "sseLog");
            if log.contains("delta:tock") || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        server.join().expect("server exits");
        assert_eq!(
            global_string(&mut ctx, "sseLog"),
            "open:1,msg:tick,delta:tock"
        );
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
