use std::env;
use std::fs;
use std::path::Path;

use serde_json::Value;

#[test]
fn html5lib_tokenizer_smoke() {
    let base = env::var("HTML5LIB_TESTS_DIR")
        .unwrap_or_else(|_| "silksurf-extras/html5lib-tests/tokenizer".to_string());
    let test_path = Path::new(&base).join("test1.test");
    if !test_path.exists() {
        eprintln!("html5lib tests not found at {}", test_path.display());
        return;
    }

    let data = fs::read_to_string(&test_path).expect("read html5lib test file");
    let value: Value = serde_json::from_str(&data).expect("parse html5lib JSON");
    assert!(value.get("tests").is_some());
}
