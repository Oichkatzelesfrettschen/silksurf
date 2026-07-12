//! `$DONE` async-completion hook.
//!
//! `$DONE()` signals a successful asynchronous run, `$DONE(error)` a failure,
//! and a second call is itself a failure. `drive_until_done` pumps the
//! microtask queue and host timer callbacks until `$DONE` fires or no runnable
//! work remains, so an embedder without a live event loop can run a
//! promise/`setTimeout`-based async test to completion.

use std::time::Duration;

use silksurf_js::{AsyncCompletion, SilkContext};

const BUDGET: Duration = Duration::from_secs(2);

#[test]
fn synchronous_done_reports_passed() {
    let mut ctx = SilkContext::new();
    ctx.eval("$DONE();").expect("script runs");
    assert_eq!(ctx.async_completion(), AsyncCompletion::Passed);
    assert_eq!(ctx.drive_until_done(BUDGET), AsyncCompletion::Passed);
}

#[test]
fn done_from_resolved_promise_passes_after_driving() {
    let mut ctx = SilkContext::new();
    ctx.eval("Promise.resolve().then(function () { $DONE(); });")
        .expect("script runs");
    // Whether the reaction runs during eval or on the first drive, the async
    // run completes successfully once the microtask queue is pumped.
    assert_eq!(ctx.drive_until_done(BUDGET), AsyncCompletion::Passed);
}

#[test]
fn done_from_set_timeout_passes_after_deadline() {
    let mut ctx = SilkContext::new();
    ctx.eval("setTimeout(function () { $DONE(); }, 30);")
        .expect("script runs");
    assert_eq!(ctx.async_completion(), AsyncCompletion::Pending);
    assert_eq!(ctx.drive_until_done(BUDGET), AsyncCompletion::Passed);
}

#[test]
fn done_from_chained_timers_and_promises_passes() {
    let mut ctx = SilkContext::new();
    ctx.eval(
        "setTimeout(function () { \
           Promise.resolve().then(function () { \
             setTimeout(function () { $DONE(); }, 10); \
           }); \
         }, 10);",
    )
    .expect("script runs");
    assert_eq!(ctx.drive_until_done(BUDGET), AsyncCompletion::Passed);
}

#[test]
fn done_with_error_reports_failure_message() {
    let mut ctx = SilkContext::new();
    ctx.eval("$DONE(new Error('boom'));").expect("script runs");
    match ctx.async_completion() {
        AsyncCompletion::Failed(message) => assert_eq!(message, "Error: boom"),
        other => panic!("expected failure, got {other:?}"),
    }
}

#[test]
fn done_with_truthy_non_error_reports_failure() {
    let mut ctx = SilkContext::new();
    ctx.eval("$DONE('not ok');").expect("script runs");
    match ctx.async_completion() {
        AsyncCompletion::Failed(message) => assert_eq!(message, "not ok"),
        other => panic!("expected failure, got {other:?}"),
    }
}

#[test]
fn double_done_is_a_failure() {
    let mut ctx = SilkContext::new();
    ctx.eval("$DONE(); $DONE();").expect("script runs");
    match ctx.async_completion() {
        AsyncCompletion::Failed(message) => {
            assert!(message.contains("more than once"), "message: {message}");
        }
        other => panic!("expected failure, got {other:?}"),
    }
}

#[test]
fn never_calling_done_stays_pending() {
    let mut ctx = SilkContext::new();
    ctx.eval("var x = 1 + 1;").expect("script runs");
    // No pending work and no $DONE call: driving returns Pending, not a hang.
    assert_eq!(ctx.drive_until_done(BUDGET), AsyncCompletion::Pending);
}

#[test]
fn reset_allows_a_second_async_run() {
    let mut ctx = SilkContext::new();
    ctx.eval("$DONE();").expect("first run");
    assert_eq!(ctx.async_completion(), AsyncCompletion::Passed);
    ctx.reset_async_completion();
    assert_eq!(ctx.async_completion(), AsyncCompletion::Pending);
    ctx.eval("setTimeout(function () { $DONE(new Error('second')); }, 5);")
        .expect("second run");
    match ctx.drive_until_done(BUDGET) {
        AsyncCompletion::Failed(message) => assert_eq!(message, "Error: second"),
        other => panic!("expected failure, got {other:?}"),
    }
}
