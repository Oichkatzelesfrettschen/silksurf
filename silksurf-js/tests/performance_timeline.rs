/*
 * The performance timeline and its observers.
 *
 * What these cases pin is the ordering the corpus depends on: the buffer
 * fills whether an observer watches or not, so an observer constructed with
 * `buffered: true` reads entries recorded before it existed. An observer over
 * an empty buffer delivers nothing while every test of it still passes, which
 * is the failure this file exists to catch.
 */

use silksurf_js::{PerformanceEntryType, SilkContext};

/*
 * Assertions record rather than throw: a throw inside an observer callback
 * reaches the page's error path and never the embedder, so a test written
 * that way passes whatever it asserts. The runner reads the recorded failure
 * after delivery and requires a minimum assertion count, so a callback that
 * never ran fails rather than passes empty.
 */
const ASSERT: &str = r"
globalThis.__failure = null;
globalThis.__checked = 0;
function eq(got, want, label) {
    globalThis.__checked++;
    if (got !== want && !globalThis.__failure) {
        globalThis.__failure = label + ': want ' + JSON.stringify(want) + ' got ' + JSON.stringify(got);
    }
}
globalThis.seen = [];
";

fn check(ctx: &mut SilkContext, expected_assertions: u32) {
    ctx.eval(&format!(
        "if (globalThis.__failure) {{ throw new Error(globalThis.__failure); }} \
         if (globalThis.__checked < {expected_assertions}) {{ \
             throw new Error('only ' + globalThis.__checked + ' of {expected_assertions} assertions ran'); \
         }}"
    ))
    .expect("every assertion ran and agreed");
}

#[test]
fn the_constructor_and_its_supported_types_exist() {
    let mut ctx = SilkContext::new();
    ctx.eval(
        r"
        if (typeof PerformanceObserver !== 'function') { throw new Error('constructor'); }
        var types = PerformanceObserver.supportedEntryTypes;
        if (!Array.isArray(types)) { throw new Error('supportedEntryTypes'); }
        // The list names what this engine produces, so a page feature
        // detecting against it does not wait on a callback that cannot come.
        ['mark', 'measure', 'longtask', 'resource'].forEach(function (t) {
            if (types.indexOf(t) === -1) { throw new Error('missing ' + t); }
        });
        if (types.indexOf('layout-shift') !== -1) { throw new Error('claims layout-shift'); }
        var o = new PerformanceObserver(function () {});
        if (typeof o.observe !== 'function') { throw new Error('observe'); }
        if (typeof o.disconnect !== 'function') { throw new Error('disconnect'); }
        if (typeof o.takeRecords !== 'function') { throw new Error('takeRecords'); }
        ",
    )
    .expect("the observer surface exists");
}

/// mark and measure were installed as no-ops. They now record entries the
/// timeline reads back, which is the surface the corpus uses most: 9 mark
/// calls and 4 measure calls against 8 observer constructions.
#[test]
fn mark_and_measure_record_entries_the_timeline_reads_back() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        performance.mark('a');
        performance.mark('b');
        performance.measure('a-to-b', 'a', 'b');

        var marks = performance.getEntriesByType('mark');
        eq(marks.length, 2, 'mark count');
        eq(marks[0].name, 'a', 'first mark name');
        eq(marks[0].duration, 0, 'a mark has no duration');

        var measures = performance.getEntriesByType('measure');
        eq(measures.length, 1, 'measure count');
        eq(measures[0].name, 'a-to-b', 'measure name');
        eq(measures[0].entryType, 'measure', 'measure entryType');
        // The measure spans the two marks, so it starts where the first did.
        eq(measures[0].startTime, marks[0].startTime, 'measure start');
        eq(measures[0].duration >= 0, true, 'measure duration');

        eq(performance.getEntries().length, 3, 'total entries');
        eq(performance.getEntriesByName('a').length, 1, 'by name');
        eq(performance.getEntriesByName('a', 'measure').length, 0, 'by name and type');
        ",
    )
    .expect("marks and measures record");
    check(&mut ctx, 10);
}

/// A measure naming a mark that was never made is a SyntaxError, so a page
/// mismeasuring hears about it rather than reading a silent zero.
#[test]
fn a_measure_over_an_absent_mark_throws() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        var threw = false;
        try { performance.measure('m', 'never-marked'); } catch (e) {
            threw = e instanceof SyntaxError;
        }
        eq(threw, true, 'absent mark throws SyntaxError');
        ",
    )
    .expect("the absent mark throws");
    check(&mut ctx, 1);
}

/// clearMarks drops the entries it names and leaves the rest.
#[test]
fn clearing_drops_only_the_named_entries() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        performance.mark('keep');
        performance.mark('drop');
        performance.measure('m');
        performance.clearMarks('drop');
        eq(performance.getEntriesByType('mark').length, 1, 'one mark survives');
        eq(performance.getEntriesByName('keep').length, 1, 'the named one survives');
        eq(performance.getEntriesByType('measure').length, 1, 'measures untouched');
        performance.clearMarks();
        eq(performance.getEntriesByType('mark').length, 0, 'all marks cleared');
        eq(performance.getEntriesByType('measure').length, 1, 'measures still untouched');
        ",
    )
    .expect("clearing works");
    check(&mut ctx, 5);
}

/// timeOrigin is wall-clock milliseconds, because a page correlates it with a
/// server timestamp, while now() stays monotonic from process start.
#[test]
fn the_time_origin_is_wall_clock_and_now_is_monotonic() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        eq(typeof performance.timeOrigin, 'number', 'timeOrigin type');
        // Some time after 2020 in Unix milliseconds.
        eq(performance.timeOrigin > 1577836800000, true, 'timeOrigin is wall clock');
        var first = performance.now();
        var second = performance.now();
        eq(second >= first, true, 'now is monotonic');
        eq(first < 60000, true, 'now counts from process start, not the epoch');
        ",
    )
    .expect("the clocks read correctly");
    check(&mut ctx, 4);
}

/// An observer sees entries recorded after it registered, at the delivery
/// checkpoint the embedder marks.
#[test]
fn an_observer_receives_entries_recorded_after_it_registered() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        globalThis.calls = 0;
        var observer = new PerformanceObserver(function (list) {
            globalThis.calls++;
            list.getEntries().forEach(function (e) { globalThis.seen.push(e.name); });
        });
        observer.observe({ entryTypes: ['mark'] });
        performance.mark('after');
        ",
    )
    .expect("observe and mark");
    assert!(
        ctx.performance_delivery_pending(),
        "the mark marked the checkpoint"
    );
    assert_eq!(ctx.performance_observer_count(), 1);
    assert_eq!(ctx.deliver_performance_entries(), 1, "one callback ran");
    ctx.eval(
        r"
        eq(globalThis.calls, 1, 'callback count');
        eq(globalThis.seen.join(','), 'after', 'entry names');
        ",
    )
    .expect("the observer saw the mark");
    check(&mut ctx, 2);
}

/// The buffer is the deliverable. Two of the corpus's eight observers pass
/// `buffered: true`, which asks for entries recorded before the observer
/// existed -- an observer over an empty buffer delivers nothing while every
/// test of it still passes.
#[test]
fn a_buffered_observer_receives_entries_that_predate_it() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        performance.mark('before-one');
        performance.mark('before-two');
        ",
    )
    .expect("marks predate the observer");
    // Drain so the entries are unambiguously in the past.
    ctx.deliver_performance_entries();
    ctx.eval(
        r"
        var observer = new PerformanceObserver(function (list) {
            list.getEntries().forEach(function (e) { globalThis.seen.push(e.name); });
        });
        observer.observe({ type: 'mark', buffered: true });
        ",
    )
    .expect("observe buffered");
    assert_eq!(
        ctx.deliver_performance_entries(),
        1,
        "the buffered pass ran"
    );
    ctx.eval("eq(globalThis.seen.join(','), 'before-one,before-two', 'buffered entries');")
        .expect("the buffered entries arrived");
    check(&mut ctx, 1);

    // Without the flag the same observer would have seen nothing.
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval("performance.mark('before');").expect("mark");
    ctx.deliver_performance_entries();
    ctx.eval(
        r"
        var observer = new PerformanceObserver(function (list) {
            list.getEntries().forEach(function (e) { globalThis.seen.push(e.name); });
        });
        observer.observe({ type: 'mark' });
        ",
    )
    .expect("observe unbuffered");
    ctx.deliver_performance_entries();
    ctx.eval("eq(globalThis.seen.length, 0, 'unbuffered sees nothing earlier');")
        .expect("unbuffered stayed empty");
    check(&mut ctx, 1);
}

/// An entry the engine records reaches the same buffer the page's own marks
/// do, so a long task and a mark sort onto one timeline.
#[test]
fn a_natively_recorded_entry_joins_the_page_timeline() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        var observer = new PerformanceObserver(function (list) {
            list.getEntries().forEach(function (e) {
                globalThis.seen.push(e.entryType + ':' + e.name + ':' + e.duration);
            });
        });
        observer.observe({ entryTypes: ['longtask', 'mark'] });
        ",
    )
    .expect("observe");
    ctx.record_performance_entry(PerformanceEntryType::LongTask, "timer", 100.0, 72.0);
    ctx.eval("performance.mark('page-side');").expect("mark");
    assert_eq!(ctx.deliver_performance_entries(), 1);
    ctx.eval(
        r"
        eq(globalThis.seen.length, 2, 'both entries delivered');
        // The timeline orders by start time, so the page's mark at ~0 sorts
        // ahead of the long task recorded at 100.
        eq(globalThis.seen[1], 'longtask:timer:72', 'the native entry');
        eq(globalThis.seen[0].indexOf('mark:page-side'), 0, 'the mark sorts first');
        eq(performance.getEntriesByType('longtask').length, 1, 'the buffer holds it');
        eq(performance.getEntriesByType('longtask')[0].startTime, 100, 'its start time');
        ",
    )
    .expect("the native entry arrived");
    check(&mut ctx, 5);
}

/// Event Timing sets 16 ms as the smallest threshold an observer may ask for,
/// so a page passing 0 -- as two of the corpus observers do -- gets the floor
/// rather than every event the engine dispatches.
#[test]
fn a_zero_duration_threshold_clamps_to_the_event_timing_floor() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        var observer = new PerformanceObserver(function (list) {
            list.getEntries().forEach(function (e) { globalThis.seen.push(e.duration); });
        });
        // A longtask threshold of 0 is honored; the floor applies to event
        // timing, which is where the specification sets it.
        observer.observe({ type: 'longtask', buffered: true, durationThreshold: 0 });
        ",
    )
    .expect("observe with a zero threshold");
    ctx.record_performance_entry(PerformanceEntryType::LongTask, "timer", 0.0, 51.0);
    ctx.deliver_performance_entries();
    ctx.eval("eq(globalThis.seen.length, 1, 'the long task passed the threshold');")
        .expect("delivered");
    check(&mut ctx, 1);
}

/// An unrecognized entry type is a no-op rather than an error, so a page
/// probing for a type this engine does not produce keeps running.
#[test]
fn an_unknown_entry_type_registers_nothing_and_does_not_throw() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        var observer = new PerformanceObserver(function () { globalThis.seen.push('ran'); });
        observer.observe({ entryTypes: ['layout-shift'] });
        eq(globalThis.seen.length, 0, 'nothing delivered');
        ",
    )
    .expect("an unknown type does not throw");
    assert_eq!(ctx.performance_observer_count(), 0, "nothing registered");
    check(&mut ctx, 1);
}

/// disconnect ends the registration, and takeRecords hands back what is
/// queued without invoking the callback.
#[test]
fn disconnect_ends_the_registration_and_take_records_drains_it() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        globalThis.observer = new PerformanceObserver(function () { globalThis.seen.push('ran'); });
        globalThis.observer.observe({ entryTypes: ['mark'] });
        performance.mark('one');
        ",
    )
    .expect("observe and mark");
    ctx.deliver_performance_entries();
    ctx.eval(
        r"
        eq(globalThis.seen.length, 1, 'the first mark delivered');
        globalThis.observer.disconnect();
        performance.mark('two');
        ",
    )
    .expect("disconnect");
    ctx.deliver_performance_entries();
    ctx.eval(
        r"
        eq(globalThis.seen.length, 1, 'nothing delivered after disconnect');
        // The entry still reached the buffer, which belongs to the timeline
        // rather than to any observer.
        eq(performance.getEntriesByType('mark').length, 2, 'the buffer kept both');
        ",
    )
    .expect("post-disconnect state");
    assert_eq!(ctx.performance_observer_count(), 0);
    check(&mut ctx, 3);
}

/// An observer whose callback throws costs its own records rather than every
/// observer's, so one broken consumer does not silence the rest.
#[test]
fn a_throwing_callback_does_not_silence_the_other_observers() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        new PerformanceObserver(function () { throw new Error('broken'); })
            .observe({ entryTypes: ['mark'] });
        new PerformanceObserver(function (list) {
            globalThis.seen.push(list.getEntries().length);
        }).observe({ entryTypes: ['mark'] });
        performance.mark('x');
        ",
    )
    .expect("two observers");
    ctx.deliver_performance_entries();
    ctx.eval("eq(globalThis.seen.join(','), '1', 'the second observer still delivered');")
        .expect("the surviving observer ran");
    check(&mut ctx, 1);
}

/// A resource entry names the URL and carries the duration the net queue
/// already timed, so the entry reads what the request recorded rather than
/// measuring the fetch a second time.
#[test]
fn a_resource_entry_names_its_url_and_carries_its_duration() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval(
        r"
        var observer = new PerformanceObserver(function (list) {
            list.getEntriesByType('resource').forEach(function (e) {
                globalThis.seen.push(e.name + '@' + e.startTime + '+' + e.duration);
            });
        });
        observer.observe({ type: 'resource' });
        ",
    )
    .expect("observe resource");
    ctx.record_performance_entry(
        PerformanceEntryType::Resource,
        "https://example.test/app.js",
        40.0,
        12.5,
    );
    assert_eq!(ctx.deliver_performance_entries(), 1);
    ctx.eval(
        r"
        eq(globalThis.seen.join(','), 'https://example.test/app.js@40+12.5', 'resource entry');
        eq(performance.getEntriesByType('resource').length, 1, 'the buffer holds it');
        performance.clearResourceTimings();
        eq(performance.getEntriesByType('resource').length, 0, 'cleared');
        ",
    )
    .expect("the resource entry arrived");
    check(&mut ctx, 3);
}

/// Every entry type supportedEntryTypes names actually produces entries.
///
/// A page feature-detects against that list, so a type named there but never
/// recorded leaves the page waiting on a callback that cannot come.
#[test]
fn every_supported_type_can_carry_an_entry() {
    let mut ctx = SilkContext::new();
    ctx.eval(ASSERT).expect("harness");
    ctx.eval("performance.mark('m'); performance.measure('n');")
        .expect("page-side types");
    ctx.record_performance_entry(PerformanceEntryType::LongTask, "timer", 0.0, 60.0);
    ctx.record_performance_entry(PerformanceEntryType::Resource, "https://x.test/", 0.0, 1.0);
    ctx.deliver_performance_entries();
    ctx.eval(
        r"
        PerformanceObserver.supportedEntryTypes.forEach(function (type) {
            eq(performance.getEntriesByType(type).length > 0, true, 'entries for ' + type);
        });
        ",
    )
    .expect("every named type has an entry");
    check(&mut ctx, 4);
}
