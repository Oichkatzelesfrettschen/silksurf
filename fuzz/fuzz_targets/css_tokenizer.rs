#![no_main]

use libfuzzer_sys::fuzz_target;
use silksurf_css::CssTokenizer;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        let mut tokenizer = CssTokenizer::new();
        let _ = tokenizer.feed(input);
        let _ = tokenizer.finish();
    }
});
