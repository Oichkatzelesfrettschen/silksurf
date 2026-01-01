#![no_main]

use libfuzzer_sys::fuzz_target;
use silksurf_html::{Tokenizer, TreeBuilder};

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let mut tokenizer = Tokenizer::new();
    let mut tokens = match tokenizer.feed(&input) {
        Ok(tokens) => tokens,
        Err(_) => return,
    };
    if let Ok(remaining) = tokenizer.finish() {
        tokens.extend(remaining);
    }
    let mut builder = TreeBuilder::new();
    let _ = builder.process_tokens(tokens);
});
