#![no_main]

use libfuzzer_sys::fuzz_target;
use silksurf_css::parse_stylesheet;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let _ = parse_stylesheet(&input);
});
