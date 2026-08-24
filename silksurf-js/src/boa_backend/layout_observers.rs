/*
 * The layout observation checkpoint.
 *
 * ResizeObserver and IntersectionObserver both report the geometry of a frame
 * rather than a tree mutation, so both read the same border box the geometry
 * provider answers and both deliver at the same point: after the frame's
 * geometry becomes current. A layout that completed and a scroll that moved
 * the presented bitmap each make it current, which is why the embedder marks
 * the checkpoint through `SilkContext::request_layout_observation` at both
 * rather than hooking the layout alone.
 *
 * Two counters gate the cost. `count` holds the live observation total, set
 * by the JS half whenever observe or unobserve moves it, so a page with no
 * observer pays one `Cell` read per frame and never reaches the JS context.
 * `pending` holds whether the geometry moved since the last delivery, so a
 * frame that changed nothing pays the same read.
 *
 * The JS half owns the registry and the change detection, because both read
 * the option bag the page passed and the option bag is a JS object.
 */

use std::{cell::Cell, rc::Rc};

use boa_engine::{Context, JsValue, NativeFunction, js_string};

/// Global naming the JS half's delivery entry point.
const DELIVER: &str = "__silksurfDeliverLayoutObservations";

/// Live observation count, written by the JS half's observe and unobserve.
pub(super) type ObservationCount = Rc<Cell<usize>>;

/// Whether the frame's geometry moved since the last delivery.
pub(super) type ObservationPending = Rc<Cell<bool>>;

/// Install the natives and the observer bootstrap.
pub(super) fn install(ctx: &mut Context, count: &ObservationCount, pending: &ObservationPending) {
    let count_handle = Rc::clone(count);
    // SAFETY: the closure captures two Rc<Cell> handles and no JS value, so
    // the garbage collector traces nothing through it.
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
        js_string!("__silksurfSetLayoutObservationCount"),
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
    let _ =
        ctx.register_global_callable(js_string!("__silksurfRequestLayoutObservation"), 0, request);

    if let Err(err) = ctx.eval(boa_engine::Source::from_bytes(BOOTSTRAP.as_bytes())) {
        eprintln!("silksurf-js: layout observer bootstrap failed: {err}");
    }
}

/// Run the JS half's delivery pass and report how many callbacks it invoked.
/// A callback that throws is reported through the page's error path and the
/// remaining observers still deliver, so one broken observer costs its own
/// records rather than every observer's.
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

/*
 * The observation registry and ResizeObserver.
 *
 * An element box arrives as twelve numbers: the border box, the four border
 * widths, then the four padding widths. The padding box is the border box
 * less the borders and the content box is that less the paddings, which is
 * what lets one read answer contentRect, contentBoxSize, and borderBoxSize
 * together.
 *
 * An observation reports once when it is first observed regardless of change,
 * which is what a lazy-loading page depends on: a static element that never
 * resizes still reaches its callback. `reported` is what distinguishes the
 * first pass from a later one, so seeding the recorded size at observe time
 * would silence it.
 */
const BOOTSTRAP: &str = r"
(function () {
    'use strict';
    var observations = [];

    function boxOf(el) {
        if (typeof __silksurfElementBox !== 'function') { return null; }
        var id = el && typeof el.nodeId === 'number' ? el.nodeId : -1;
        return id < 0 ? null : __silksurfElementBox(id);
    }

    // The three boxes one read answers. An element the layout produced no box
    // for measures zero, which is what a browser reports for a display:none
    // subtree.
    function boxes(el) {
        var b = boxOf(el) || [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        var padW = b[2] - b[7] - b[5];
        var padH = b[3] - b[4] - b[6];
        return {
            x: b[0], y: b[1],
            borderWidth: b[2], borderHeight: b[3],
            paddingLeft: b[11], paddingTop: b[8],
            contentWidth: Math.max(0, padW - b[11] - b[9]),
            contentHeight: Math.max(0, padH - b[8] - b[10]),
        };
    }

    function size(kind, m) {
        return kind === 'border-box'
            ? [m.borderWidth, m.borderHeight]
            : [m.contentWidth, m.contentHeight];
    }

    function entryFor(observation, m) {
        return {
            target: observation.target,
            contentRect: {
                x: m.paddingLeft, y: m.paddingTop,
                left: m.paddingLeft, top: m.paddingTop,
                width: m.contentWidth, height: m.contentHeight,
                right: m.paddingLeft + m.contentWidth,
                bottom: m.paddingTop + m.contentHeight,
                toJSON: function () { return this; },
            },
            borderBoxSize: [{ inlineSize: m.borderWidth, blockSize: m.borderHeight }],
            contentBoxSize: [{ inlineSize: m.contentWidth, blockSize: m.contentHeight }],
        };
    }

    globalThis.__silksurfLayoutObservations = observations;
    globalThis.__silksurfSyncLayoutObservationCount = function () {
        __silksurfSetLayoutObservationCount(observations.length);
    };

    // Every observer's pending entries collect before any callback runs, so a
    // callback that observes another element queues it for the next pass
    // rather than extending this one.
    globalThis.__silksurfDeliverLayoutObservations = function () {
        var pending = [];
        for (var i = 0; i < observations.length; i++) {
            var observation = observations[i];
            if (observation.kind !== 'resize') { continue; }
            var m = boxes(observation.target);
            var next = size(observation.box, m);
            if (observation.reported && next[0] === observation.width && next[1] === observation.height) { continue; }
            observation.reported = true;
            observation.width = next[0];
            observation.height = next[1];
            var slot = pending.indexOf(observation.observer);
            if (slot === -1) { pending.push(observation.observer); observation.observer._queue = []; }
            observation.observer._queue.push(entryFor(observation, m));
        }
        var ran = 0;
        for (var j = 0; j < pending.length; j++) {
            var observer = pending[j];
            var entries = observer._queue;
            observer._queue = [];
            if (!entries.length) { continue; }
            ran++;
            try { observer._callback.call(observer, entries, observer); }
            catch (e) { globalThis.reportError ? reportError(e) : console.error(String(e)); }
        }
        return ran;
    };

    function ResizeObserver(callback) {
        if (typeof callback !== 'function') { throw new TypeError('ResizeObserver requires a callback'); }
        this._callback = callback;
        this._queue = [];
    }
    ResizeObserver.prototype.observe = function (target, options) {
        if (!target) { throw new TypeError('ResizeObserver.observe requires a target'); }
        // device-pixel-content-box observes the content box here, because the
        // engine presents one device pixel per CSS pixel.
        var box = (options && options.box) === 'border-box' ? 'border-box' : 'content-box';
        for (var i = 0; i < observations.length; i++) {
            var found = observations[i];
            if (found.observer !== this || found.target !== target) { continue; }
            // Re-observing with a different box restarts the observation, so
            // the new box reports once before any change.
            if (found.box !== box) { found.box = box; found.reported = false; }
            __silksurfRequestLayoutObservation();
            return;
        }
        observations.push({
            kind: 'resize', observer: this, target: target, box: box,
            reported: false, width: 0, height: 0,
        });
        __silksurfSyncLayoutObservationCount();
        __silksurfRequestLayoutObservation();
    };
    ResizeObserver.prototype.unobserve = function (target) {
        for (var i = observations.length - 1; i >= 0; i--) {
            var found = observations[i];
            if (found.observer === this && found.target === target) { observations.splice(i, 1); }
        }
        __silksurfSyncLayoutObservationCount();
    };
    ResizeObserver.prototype.disconnect = function () {
        for (var i = observations.length - 1; i >= 0; i--) {
            if (observations[i].observer === this) { observations.splice(i, 1); }
        }
        this._queue = [];
        __silksurfSyncLayoutObservationCount();
    };
    Object.defineProperty(ResizeObserver.prototype, Symbol.toStringTag, {
        configurable: true, value: 'ResizeObserver',
    });
    globalThis.ResizeObserver = ResizeObserver;
})();
";
