use silksurf_engine::{JsRuntime, JsTask, NoopJsRuntime};
use std::time::Instant;

fn main() {
    let mut runtime = NoopJsRuntime::new();
    let iterations = 100_000;
    let start = Instant::now();
    for _ in 0..iterations {
        runtime.enqueue_task(JsTask::Script("1 + 1".into()));
    }
    runtime.run_microtasks().expect("run microtasks");
    let elapsed = start.elapsed();
    println!("js tasks: {}", iterations);
    println!("total: {:?}", elapsed);
}
