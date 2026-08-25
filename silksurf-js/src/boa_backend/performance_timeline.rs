/*
 * The performance timeline and its observers.
 *
 * The buffer is the deliverable rather than the observer. Two of the corpus's
 * eight observers construct with `buffered: true`, which asks for entries
 * recorded before the observer existed, so the timeline has to fill from the
 * time origin whether any observer watches it or not. An observer built over
 * an empty buffer delivers nothing while every test of it still passes.
 *
 * Entries come from two directions. `mark` and `measure` are the page's own,
 * and the JS half owns them because a page reads them back as objects. The
 * rest come from instrumentation the engine already carries: `run_host_callbacks`
 * times every timer and animation-frame callback and the job drain, which is
 * what `longtask` measures, and the net queue knows when each fetch started
 * and finished, which is what `resource` measures. Those call sites gain a
 * second consumer rather than new measurement.
 *
 * Native entries cross into JS at the same checkpoint the layout observers
 * deliver at: `pending` says whether anything was recorded since the last
 * pass and `count` holds the live observer total, so a page observing nothing
 * pays two `Cell` reads per frame and never reaches the JS context.
 */

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use boa_engine::{
    Context, JsValue, NativeFunction, js_string,
    object::{JsObject, builtins::JsArray},
    property::Attribute,
};

/// Global naming the JS half's delivery entry point.
const DELIVER: &str = "__silksurfDeliverPerformanceEntries";

/// One entry the engine recorded, awaiting handoff to the JS buffer.
pub(super) struct NativeEntry {
    pub(super) entry_type: &'static str,
    pub(super) name: String,
    /// Milliseconds since the same epoch `performance.now()` counts from.
    pub(super) start_ms: f64,
    pub(super) duration_ms: f64,
}

/// Entries recorded by the engine since the last handoff.
pub(super) type EntrySink = Rc<RefCell<Vec<NativeEntry>>>;

/// Live observer count, written by the JS half's observe and disconnect.
pub(super) type ObserverCount = Rc<Cell<usize>>;

/// Whether any entry was recorded since the last delivery.
pub(super) type EntryPending = Rc<Cell<bool>>;

/// Records one entry against the timeline.
///
/// The buffer fills whether an observer watches or not, because
/// `getEntriesByType` and a `buffered` observer both read entries that
/// predate them. `pending` gates only the delivery pass.
pub(super) fn record(
    sink: &EntrySink,
    pending: &EntryPending,
    entry_type: &'static str,
    name: impl Into<String>,
    start_ms: f64,
    duration_ms: f64,
) {
    sink.borrow_mut().push(NativeEntry {
        entry_type,
        name: name.into(),
        start_ms,
        duration_ms,
    });
    pending.set(true);
}

/// The duration below which a callback is not a long task.
///
/// The Long Tasks specification defines a long task as one running at least
/// 50 ms, so a shorter callback records nothing and the buffer holds the
/// tasks a page would act on rather than every callback it ran.
pub(super) const LONG_TASK_THRESHOLD_MS: f64 = 50.0;

/// Install the natives and the timeline bootstrap.
pub(super) fn install(
    ctx: &mut Context,
    sink: &EntrySink,
    count: &ObserverCount,
    pending: &EntryPending,
    time_origin_ms: f64,
) {
    let sink_handle = Rc::clone(sink);
    // SAFETY: the closure captures one Rc<RefCell<Vec<NativeEntry>>> holding
    // plain data and no JS value, so the garbage collector traces nothing
    // through it.
    let drain = unsafe {
        NativeFunction::from_closure(move |_this, _args, ctx| {
            let entries = std::mem::take(&mut *sink_handle.borrow_mut());
            let array = JsArray::new(ctx);
            for entry in entries {
                let object = JsObject::with_object_proto(ctx.intrinsics());
                object.set(
                    js_string!("entryType"),
                    js_string!(entry.entry_type),
                    false,
                    ctx,
                )?;
                object.set(js_string!("name"), js_string!(entry.name), false, ctx)?;
                object.set(
                    js_string!("startTime"),
                    JsValue::from(entry.start_ms),
                    false,
                    ctx,
                )?;
                object.set(
                    js_string!("duration"),
                    JsValue::from(entry.duration_ms),
                    false,
                    ctx,
                )?;
                array.push(JsValue::from(object), ctx)?;
            }
            Ok(array.into())
        })
    };
    let _ = ctx.register_global_callable(js_string!("__silksurfDrainNativeEntries"), 0, drain);

    let count_handle = Rc::clone(count);
    // SAFETY: the closure captures one Rc<Cell> handle and no JS value.
    let set_count = unsafe {
        NativeFunction::from_closure(move |_this, args, ctx| {
            let total = args
                .first()
                .map(|value| value.to_u32(ctx))
                .transpose()?
                .unwrap_or(0);
            count_handle.set(total as usize);
            Ok(JsValue::undefined())
        })
    };
    let _ = ctx.register_global_callable(
        js_string!("__silksurfSetPerformanceObserverCount"),
        1,
        set_count,
    );

    let pending_handle = Rc::clone(pending);
    // SAFETY: the closure captures one Rc<Cell> handle and no JS value.
    let request = unsafe {
        NativeFunction::from_closure(move |_this, _args, _ctx| {
            pending_handle.set(true);
            Ok(JsValue::undefined())
        })
    };
    let _ = ctx.register_global_callable(
        js_string!("__silksurfRequestPerformanceDelivery"),
        0,
        request,
    );

    let global = ctx.global_object();
    if let Ok(performance) = global.get(js_string!("performance"), ctx)
        && let Some(object) = performance.as_object()
    {
        let _ = object.define_property_or_throw(
            js_string!("timeOrigin"),
            boa_engine::property::PropertyDescriptor::builder()
                .value(JsValue::from(time_origin_ms))
                .writable(false)
                .enumerable(true)
                .configurable(false),
            ctx,
        );
    }
    let _ = ctx.register_global_property(
        js_string!("__silksurfLongTaskThreshold"),
        JsValue::from(LONG_TASK_THRESHOLD_MS),
        Attribute::empty(),
    );

    if let Err(err) = ctx.eval(boa_engine::Source::from_bytes(BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: performance timeline bootstrap failed: {err}");
    }
}

/// Run the JS half's delivery pass and report how many callbacks it invoked.
pub(super) fn deliver(ctx: &mut Context) -> usize {
    let global = ctx.global_object();
    let Ok(deliver) = global.get(js_string!(DELIVER), ctx) else {
        return 0;
    };
    let Some(callable) = deliver.as_callable() else {
        return 0;
    };
    let Ok(result) = callable.call(&JsValue::undefined(), &[], ctx) else {
        return 0;
    };
    result.as_number().unwrap_or(0.0) as usize
}

/// The JS half of the timeline.
///
/// The buffer, the entry objects, and the observer registry live here because
/// a page reads every one of them as a JS value. The Rust half contributes
/// entries the engine measured and the checkpoint that delivers them.
const BOOTSTRAP: &str = r"
(function () {
    var entries = [];
    /// Entries recorded since the last delivery, awaiting routing.
    ///
    /// The routing set is tracked rather than derived from the buffer,
    /// because clearMarks and clearMeasures splice the buffer and an index
    /// into it stops naming the same entries.
    var undelivered = [];
    var observations = [];
    var nextObserverId = 1;

    /// Entry types this engine actually produces.
    ///
    /// A page feature-detects against this list, so naming a type that never
    /// yields an entry leaves it waiting for a callback that cannot come.
    var SUPPORTED = ['mark', 'measure', 'longtask', 'resource'];

    function entry(type, name, startTime, duration, detail) {
        return {
            entryType: type,
            name: String(name),
            startTime: startTime,
            duration: duration,
            detail: detail === undefined ? null : detail,
            toJSON: function () {
                return {
                    entryType: this.entryType,
                    name: this.name,
                    startTime: this.startTime,
                    duration: this.duration
                };
            }
        };
    }

    function append(record) {
        entries.push(record);
        undelivered.push(record);
        __silksurfRequestPerformanceDelivery();
        return record;
    }

    function syncObserverCount() {
        __silksurfSetPerformanceObserverCount(observations.length);
    }

    // -- the timeline --------------------------------------------------------

    performance.mark = function (name, options) {
        var startTime = performance.now();
        var detail;
        if (options && typeof options === 'object') {
            if (typeof options.startTime === 'number') { startTime = options.startTime; }
            detail = options.detail;
        }
        return append(entry('mark', name, startTime, 0, detail));
    };

    /// Resolves one end of a measure to a time.
    ///
    /// A name resolves to the most recent mark carrying it, which is what
    /// lets a page measure between two marks it made earlier.
    function resolveTime(value, fallback) {
        if (typeof value === 'number') { return value; }
        if (typeof value === 'string') {
            for (var i = entries.length - 1; i >= 0; i--) {
                if (entries[i].entryType === 'mark' && entries[i].name === value) {
                    return entries[i].startTime;
                }
            }
            throw new SyntaxError('no mark named ' + value);
        }
        return fallback;
    }

    performance.measure = function (name, startOrOptions, endMark) {
        var start = 0;
        var end = performance.now();
        var detail;
        if (startOrOptions && typeof startOrOptions === 'object') {
            detail = startOrOptions.detail;
            if (startOrOptions.start !== undefined) {
                start = resolveTime(startOrOptions.start, 0);
            }
            if (startOrOptions.end !== undefined) {
                end = resolveTime(startOrOptions.end, end);
            } else if (typeof startOrOptions.duration === 'number') {
                end = start + startOrOptions.duration;
            }
        } else {
            if (startOrOptions !== undefined) { start = resolveTime(startOrOptions, 0); }
            if (endMark !== undefined) { end = resolveTime(endMark, end); }
        }
        return append(entry('measure', name, start, end - start, detail));
    };

    performance.getEntries = function () {
        return entries.slice();
    };

    performance.getEntriesByType = function (type) {
        return entries.filter(function (record) { return record.entryType === type; });
    };

    performance.getEntriesByName = function (name, type) {
        return entries.filter(function (record) {
            return record.name === name && (type === undefined || record.entryType === type);
        });
    };

    function clearByType(type, name) {
        entries = entries.filter(function (record) {
            if (record.entryType !== type) { return true; }
            return name !== undefined && record.name !== name;
        });
    }

    performance.clearMarks = function (name) { clearByType('mark', name); };
    performance.clearMeasures = function (name) { clearByType('measure', name); };
    performance.clearResourceTimings = function () { clearByType('resource', undefined); };

    // -- the observer --------------------------------------------------------

    function PerformanceObserverEntryList(records) {
        this._records = records;
    }
    PerformanceObserverEntryList.prototype.getEntries = function () {
        return this._records.slice();
    };
    PerformanceObserverEntryList.prototype.getEntriesByType = function (type) {
        return this._records.filter(function (r) { return r.entryType === type; });
    };
    PerformanceObserverEntryList.prototype.getEntriesByName = function (name, type) {
        return this._records.filter(function (r) {
            return r.name === name && (type === undefined || r.entryType === type);
        });
    };

    function PerformanceObserver(callback) {
        if (typeof callback !== 'function') {
            throw new TypeError('PerformanceObserver requires a callback');
        }
        this._callback = callback;
        this._id = nextObserverId++;
        this._queue = [];
    }

    /// The floor a duration threshold clamps to.
    ///
    /// Event Timing sets 16 ms as the smallest threshold an observer may ask
    /// for, so a page passing 0 -- as two of the corpus observers do -- gets
    /// the floor rather than every event the engine dispatches.
    function thresholdFor(type, requested) {
        if (typeof requested !== 'number') { return 0; }
        if (type === 'event' || type === 'first-input') { return Math.max(16, requested); }
        return Math.max(0, requested);
    }

    PerformanceObserver.prototype.observe = function (options) {
        options = options || {};
        var types = [];
        if (options.entryTypes) {
            if (options.type !== undefined) {
                throw new TypeError('observe takes entryTypes or type, not both');
            }
            types = Array.prototype.slice.call(options.entryTypes);
        } else if (options.type !== undefined) {
            types = [options.type];
        } else {
            throw new TypeError('observe requires entryTypes or type');
        }
        var known = types.filter(function (t) { return SUPPORTED.indexOf(t) !== -1; });
        if (known.length === 0) {
            // Every requested type is unrecognized, which the spec makes a
            // no-op rather than an error so a page probing for one it does
            // not get keeps running.
            return;
        }
        var self = this;
        // A second observe replaces this observer's registration, matching
        // the single-registration model entryTypes uses.
        observations = observations.filter(function (o) { return o.observer !== self; });
        observations.push({
            observer: this,
            types: known,
            threshold: thresholdFor(types[0], options.durationThreshold)
        });
        syncObserverCount();

        if (options.buffered) {
            // The buffer predates this observer, which is the whole point of
            // the flag: entries recorded before it existed reach it now.
            var buffered = entries.filter(function (r) {
                return known.indexOf(r.entryType) !== -1;
            });
            if (buffered.length > 0) {
                this._queue = this._queue.concat(buffered);
                __silksurfRequestPerformanceDelivery();
            }
        }
    };

    PerformanceObserver.prototype.disconnect = function () {
        var self = this;
        observations = observations.filter(function (o) { return o.observer !== self; });
        this._queue = [];
        syncObserverCount();
    };

    PerformanceObserver.prototype.takeRecords = function () {
        var taken = this._queue;
        this._queue = [];
        return taken;
    };

    Object.defineProperty(PerformanceObserver, 'supportedEntryTypes', {
        get: function () { return SUPPORTED.slice(); }
    });

    globalThis.PerformanceObserver = PerformanceObserver;
    globalThis.PerformanceObserverEntryList = PerformanceObserverEntryList;

    // -- delivery ------------------------------------------------------------

    globalThis.__silksurfDeliverPerformanceEntries = function () {
        var native = __silksurfDrainNativeEntries();
        for (var i = 0; i < native.length; i++) {
            var record = native[i];
            var made = entry(
                record.entryType, record.name, record.startTime, record.duration, undefined
            );
            entries.push(made);
            undelivered.push(made);
        }
        // Route each entry recorded since the last pass to the observers
        // watching its type. A queue already holding buffered entries keeps
        // them, so an observer that just constructed delivers them together.
        // The Performance Timeline orders entries by start time, so an entry
        // the engine recorded mid-frame sorts against the page's own marks
        // rather than after whichever arrived last.
        var fresh = undelivered.sort(function (a, b) { return a.startTime - b.startTime; });
        undelivered = [];
        for (var o = 0; o < observations.length; o++) {
            var observation = observations[o];
            for (var e = 0; e < fresh.length; e++) {
                if (observation.types.indexOf(fresh[e].entryType) === -1) { continue; }
                if (fresh[e].duration < observation.threshold) { continue; }
                observation.observer._queue.push(fresh[e]);
            }
        }
        var ran = 0;
        for (var j = 0; j < observations.length; j++) {
            var target = observations[j].observer;
            if (target._queue.length === 0) { continue; }
            var records = target.takeRecords();
            try {
                target._callback(new PerformanceObserverEntryList(records), target);
            } catch (error) {
                if (typeof reportError === 'function') { reportError(error); }
            }
            ran++;
        }
        return ran;
    };
})();
";
