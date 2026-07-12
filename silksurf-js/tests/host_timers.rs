//! Deadline semantics for setTimeout / setInterval host callbacks.
//!
//! HTML timers order by deadline, not registration: a callback registered
//! with a delay must not run before that delay elapses, and the scheduler
//! must expose the earliest deadline so an event loop can sleep until it.

use std::time::{Duration, Instant};

use silksurf_js::SilkContext;

#[test]
fn set_timeout_delay_defers_execution_until_deadline() {
    let mut ctx = SilkContext::new();
    ctx.eval("var fired = 0; setTimeout(function () { fired += 1; }, 40);")
        .expect("script evaluates");

    assert!(ctx.has_pending_host_callbacks());
    let ran = ctx
        .run_host_callbacks(16)
        .expect("immediate drain succeeds");
    assert_eq!(ran, 0, "callback must not fire before its 40ms deadline");
    ctx.eval("if (fired !== 0) { throw new Error('fired early'); }")
        .expect("callback has not fired yet");

    let deadline = ctx
        .next_host_callback_deadline()
        .expect("a timer is scheduled");
    assert!(deadline <= Instant::now() + Duration::from_millis(45));

    std::thread::sleep(Duration::from_millis(50));
    let ran = ctx.run_host_callbacks(16).expect("due drain succeeds");
    assert_eq!(ran, 1, "callback fires once its deadline passes");
    ctx.eval("if (fired !== 1) { throw new Error('expected one firing'); }")
        .expect("callback fired exactly once");
    assert!(!ctx.has_pending_host_callbacks());
}

#[test]
fn zero_delay_timers_fire_in_registration_order() {
    let mut ctx = SilkContext::new();
    ctx.eval(
        "var order = []; \
         setTimeout(function () { order.push('a'); }); \
         setTimeout(function () { order.push('b'); }, 0);",
    )
    .expect("script evaluates");
    let ran = ctx.run_host_callbacks(16).expect("drain succeeds");
    assert_eq!(ran, 2);
    ctx.eval("if (order.join('') !== 'ab') { throw new Error(order.join('')); }")
        .expect("zero-delay timers preserve registration order");
}

#[test]
fn interval_rearms_at_its_period_and_cancels() {
    let mut ctx = SilkContext::new();
    ctx.eval("var ticks = 0; var id = setInterval(function () { ticks += 1; }, 10);")
        .expect("script evaluates");

    std::thread::sleep(Duration::from_millis(15));
    let ran = ctx.run_host_callbacks(16).expect("first drain succeeds");
    assert_eq!(ran, 1, "one interval firing per elapsed period");
    assert!(
        ctx.has_pending_host_callbacks(),
        "interval stays scheduled after firing"
    );

    ctx.eval("clearInterval(id);").expect("cancel evaluates");
    assert!(!ctx.has_pending_host_callbacks());
    assert!(ctx.next_host_callback_deadline().is_none());
}

#[test]
fn negative_and_missing_delays_clamp_to_immediate() {
    let mut ctx = SilkContext::new();
    ctx.eval(
        "var fired = 0; \
         setTimeout(function () { fired += 1; }, -5); \
         setTimeout(function () { fired += 1; });",
    )
    .expect("script evaluates");
    let deadline = ctx
        .next_host_callback_deadline()
        .expect("timers are scheduled");
    assert!(deadline <= Instant::now());
    let ran = ctx.run_host_callbacks(16).expect("drain succeeds");
    assert_eq!(ran, 2);
}
