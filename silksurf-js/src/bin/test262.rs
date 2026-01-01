//! test262 conformance test runner CLI
//!
//! Usage:
//!   test262 [OPTIONS] [PATH]
//!
//! Examples:
//!   test262                           # Run all tests
//!   test262 language/expressions      # Run expression tests
//!   test262 --verbose language/types  # Verbose output
//!   test262 --list-features           # List supported features

use std::collections::HashSet;
use std::env;
use std::path::PathBuf;
use std::time::Instant;

// Import from the test262 module in tests/
// For now, we'll create a minimal inline version

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut verbose = false;
    let mut list_features = false;
    let mut test262_path = PathBuf::from("test262");
    let mut test_path = String::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-v" | "--verbose" => verbose = true,
            "--list-features" => list_features = true,
            "--test262" => {
                i += 1;
                if i < args.len() {
                    test262_path = PathBuf::from(&args[i]);
                }
            }
            "-h" | "--help" => {
                print_help();
                return;
            }
            arg if !arg.starts_with('-') => {
                test_path = arg.to_string();
            }
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    if list_features {
        print_features();
        return;
    }

    println!("SilkSurfJS test262 Runner");
    println!("========================\n");

    // Check test262 path
    let test_dir = if test_path.is_empty() {
        test262_path.join("test")
    } else {
        test262_path.join("test").join(&test_path)
    };

    if !test_dir.exists() {
        eprintln!("Error: test262 directory not found at {:?}", test_dir);
        eprintln!("\nTo set up test262:");
        eprintln!("  git clone https://github.com/tc39/test262.git");
        eprintln!("  # or");
        eprintln!("  git submodule add https://github.com/tc39/test262.git");
        std::process::exit(1);
    }

    println!("Test directory: {:?}", test_dir);
    println!("Verbose: {}", verbose);
    println!();

    // Run tests
    let start = Instant::now();
    let results = run_tests(&test_dir, verbose);
    let duration = start.elapsed();

    // Print summary
    println!("\n{}", "=".repeat(60));
    println!("test262 Summary");
    println!("{}", "=".repeat(60));
    println!("Total:   {}", results.total);
    println!("Passed:  {} ({:.1}%)", results.passed, results.pass_rate());
    println!("Failed:  {}", results.failed);
    println!("Skipped: {}", results.skipped);
    println!("Time:    {:.2}s", duration.as_secs_f64());

    if results.failed > 0 {
        std::process::exit(1);
    }
}

fn print_help() {
    println!("SilkSurfJS test262 Conformance Test Runner");
    println!();
    println!("USAGE:");
    println!("    test262 [OPTIONS] [PATH]");
    println!();
    println!("ARGS:");
    println!("    [PATH]    Subdirectory to test (e.g., language/expressions)");
    println!();
    println!("OPTIONS:");
    println!("    -v, --verbose        Verbose output");
    println!("    --test262 <PATH>     Path to test262 repository");
    println!("    --list-features      List supported/unsupported features");
    println!("    -h, --help           Print help");
}

fn print_features() {
    println!("Supported Features:");
    for feature in SUPPORTED_FEATURES {
        println!("  [+] {}", feature);
    }
    println!();
    println!("Unsupported Features (tests skipped):");
    for feature in UNSUPPORTED_FEATURES {
        println!("  [-] {}", feature);
    }
}

// Minimal test runner (inline version)
struct TestResults {
    total: usize,
    passed: usize,
    failed: usize,
    skipped: usize,
}

impl TestResults {
    fn pass_rate(&self) -> f64 {
        let run = self.passed + self.failed;
        if run == 0 {
            0.0
        } else {
            (self.passed as f64 / run as f64) * 100.0
        }
    }
}

fn run_tests(test_dir: &std::path::Path, verbose: bool) -> TestResults {
    let mut results = TestResults {
        total: 0,
        passed: 0,
        failed: 0,
        skipped: 0,
    };

    let skip_features: HashSet<_> = UNSUPPORTED_FEATURES.iter().map(|s| s.to_string()).collect();

    collect_and_run(test_dir, &mut results, verbose, &skip_features);
    results
}

fn collect_and_run(
    dir: &std::path::Path,
    results: &mut TestResults,
    verbose: bool,
    skip_features: &HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip certain directories
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "staging" || name == "intl402" || name.starts_with('_') {
                continue;
            }
            collect_and_run(&path, results, verbose, skip_features);
        } else if path.extension().map_or(false, |e| e == "js") {
            run_single_test(&path, results, verbose, skip_features);
        }
    }
}

fn run_single_test(
    path: &std::path::Path,
    results: &mut TestResults,
    verbose: bool,
    skip_features: &HashSet<String>,
) {
    results.total += 1;

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            results.failed += 1;
            if verbose {
                eprintln!("FAIL [read error]: {:?} - {}", path, e);
            }
            return;
        }
    };

    // Parse metadata
    let metadata = parse_metadata(&content);

    // Check for unsupported features
    if let Some(features) = &metadata.features {
        for feature in features {
            if skip_features.contains(feature.as_str()) {
                results.skipped += 1;
                if verbose {
                    println!("SKIP [{}]: {:?}", feature, path);
                }
                return;
            }
        }
    }

    // Skip async and module tests for now
    if metadata.is_async || metadata.is_module {
        results.skipped += 1;
        if verbose {
            let reason = if metadata.is_async { "async" } else { "module" };
            println!("SKIP [{}]: {:?}", reason, path);
        }
        return;
    }

    // Try to lex the test
    let test_source = extract_test_source(&content);
    let outcome = run_lexer_test(&test_source, &metadata);

    match outcome {
        TestOutcome::Pass => {
            results.passed += 1;
            if verbose {
                println!("PASS: {:?}", path);
            }
        }
        TestOutcome::Fail(reason) => {
            results.failed += 1;
            if verbose {
                eprintln!("FAIL: {:?} - {}", path, reason);
            }
        }
    }
}

#[derive(Default)]
struct SimpleMetadata {
    features: Option<Vec<String>>,
    is_async: bool,
    is_module: bool,
    is_negative_parse: bool,
    negative_type: Option<String>,
}

fn parse_metadata(content: &str) -> SimpleMetadata {
    let mut meta = SimpleMetadata::default();

    // Find YAML frontmatter
    let Some(start) = content.find("/*---") else {
        return meta;
    };
    let Some(end) = content[start..].find("---*/") else {
        return meta;
    };
    let yaml = &content[start + 5..start + end];

    for line in yaml.lines() {
        let line = line.trim();
        if line.starts_with("flags:") {
            let flags = line.strip_prefix("flags:").unwrap_or("").trim();
            if flags.contains("async") {
                meta.is_async = true;
            }
            if flags.contains("module") {
                meta.is_module = true;
            }
        } else if line.starts_with("features:") {
            let features_str = line.strip_prefix("features:").unwrap_or("").trim();
            let features: Vec<String> = features_str
                .trim_matches(&['[', ']'][..])
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            meta.features = Some(features);
        } else if line.starts_with("phase:") && line.contains("parse") {
            meta.is_negative_parse = true;
        } else if line.starts_with("type:") {
            meta.negative_type = Some(
                line.strip_prefix("type:")
                    .unwrap_or("")
                    .trim()
                    .to_string(),
            );
        }
    }

    meta
}

fn extract_test_source(content: &str) -> String {
    // Remove metadata, get actual test code
    if let Some(end) = content.find("---*/") {
        content[end + 5..].to_string()
    } else {
        content.to_string()
    }
}

enum TestOutcome {
    Pass,
    Fail(String),
}

fn run_lexer_test(source: &str, metadata: &SimpleMetadata) -> TestOutcome {
    use silksurf_js::{Lexer, TokenKind};

    let lexer = Lexer::new(source);
    let mut has_error = false;
    let mut error_msg = String::new();

    for token in lexer {
        if let TokenKind::Error(e) = token.kind {
            has_error = true;
            error_msg = e.to_string();
            break;
        }
    }

    if metadata.is_negative_parse {
        // Expected to fail
        if has_error {
            TestOutcome::Pass
        } else {
            TestOutcome::Fail(format!(
                "Expected parse error {} but succeeded",
                metadata.negative_type.as_deref().unwrap_or("unknown")
            ))
        }
    } else {
        // Expected to succeed
        if has_error {
            TestOutcome::Fail(error_msg)
        } else {
            TestOutcome::Pass
        }
    }
}

// Feature lists
const SUPPORTED_FEATURES: &[&str] = &[
    "let",
    "const",
    "arrow-function",
    "class",
    "template",
    "default-parameters",
    "rest-parameters",
    "spread",
    "destructuring-binding",
    "for-of",
    "Symbol",
    "generators",
    "Promise",
    "async-functions",
    "BigInt",
    "optional-chaining",
    "nullish-coalescing",
    "numeric-separator-literal",
    "object-rest",
    "object-spread",
];

const UNSUPPORTED_FEATURES: &[&str] = &[
    "Temporal",
    "ShadowRealm",
    "decorators",
    "regexp-v-flag",
    "iterator-helpers",
    "set-methods",
    "promise-with-resolvers",
    "ArrayBuffer.prototype.transfer",
    "resizable-arraybuffer",
    "arraybuffer-transfer",
    "Atomics",
    "SharedArrayBuffer",
    "Atomics.waitAsync",
    "FinalizationRegistry",
    "WeakRef",
    "Intl.Locale",
    "Intl.ListFormat",
    "Intl.Segmenter",
    "Intl.DurationFormat",
    "Intl.DisplayNames",
    "Intl.NumberFormat-v3",
    "import.meta",
    "dynamic-import",
    "top-level-await",
    "json-modules",
    "import-assertions",
    "import-attributes",
    "Atomics.pause",
    "explicit-resource-management",
    "regexp-duplicate-named-groups",
    "symbols-as-weakmap-keys",
    "change-array-by-copy",
    "array-grouping",
    "well-formed-json-stringify",
    "String.prototype.isWellFormed",
    "String.prototype.toWellFormed",
    "Array.fromAsync",
];
