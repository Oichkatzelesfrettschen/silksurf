use silksurf_engine::{JsRuntime, JsTask, NoopJsRuntime};

#[test]
fn noop_js_runtime_executes_tasks() {
    let mut runtime = NoopJsRuntime::new();
    runtime.enqueue_task(JsTask::Script("1 + 1".into()));
    assert_eq!(runtime.pending_tasks(), 1);
    runtime.run_microtasks().unwrap();
    assert_eq!(runtime.pending_tasks(), 0);
}
