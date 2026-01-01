use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use silksurf_css::parse_stylesheet;

#[test]
fn css_harness_smoke() {
    let base = match env::var("CSS_TESTS_DIR") {
        Ok(value) => value,
        Err(_) => return,
    };
    let root = Path::new(&base);
    if !root.exists() {
        eprintln!("css tests not found at {}", root.display());
        return;
    }

    let mut files = Vec::new();
    collect_css_files(root, &mut files);
    if files.is_empty() {
        eprintln!("no css files found under {}", root.display());
        return;
    }

    for file in files {
        let data = fs::read_to_string(&file).expect("read css file");
        let _ = parse_stylesheet(&data).expect("parse css");
    }
}

fn collect_css_files(root: &Path, files: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_css_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("css") {
            files.push(path);
        }
    }
}
