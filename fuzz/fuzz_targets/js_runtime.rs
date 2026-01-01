#![no_main]

use libfuzzer_sys::fuzz_target;
use silksurf_engine::{JsRuntime, NoopJsRuntime};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let mut runtime = NoopJsRuntime::new();
        let _ = runtime.evaluate(input);
    }
});
