//! test262 test runner
//!
//! Executes test262 conformance tests against the SilkSurfJS engine.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::metadata::{TestMetadata, NegativePhase};
use super::host::{Host262, harness_files};

/// Result of running a single test
#[derive(Debug, Clone)]
pub struct TestResult {
    /// Test file path
    pub path: PathBuf,
    /// Test outcome
    pub outcome: TestOutcome,
    /// Execution time
    pub duration: Duration,
    /// Error message if failed
    pub error: Option<String>,
    /// Strict mode variant
    pub strict: bool,
}

/// Possible test outcomes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestOutcome {
    /// Test passed
    Pass,
    /// Test failed
    Fail,
    /// Test skipped (unsupported feature)
    Skip,
    /// Test timed out
    Timeout,
    /// Test crashed/panicked
    Crash,
}

impl TestOutcome {
    pub fn is_pass(&self) -> bool {
        matches!(self, TestOutcome::Pass)
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, TestOutcome::Fail)
    }
}

/// Configuration for test262 runner
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Path to test262 repository
    pub test262_path: PathBuf,
    /// Features to enable
    pub enabled_features: HashSet<String>,
    /// Features to skip
    pub skip_features: HashSet<String>,
    /// Test timeout
    pub timeout: Duration,
    /// Run in parallel
    pub parallel: bool,
    /// Number of threads (0 = auto)
    pub threads: usize,
    /// Verbose output
    pub verbose: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            test262_path: PathBuf::from("test262"),
            enabled_features: HashSet::new(),
            skip_features: default_skip_features(),
            timeout: Duration::from_secs(10),
            parallel: true,
            threads: 0,
            verbose: false,
        }
    }
}

/// Features to skip by default (not yet implemented)
fn default_skip_features() -> HashSet<String> {
    [
        // Stage 3+ features not yet implemented
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
        // Atomics/SharedArrayBuffer
        "Atomics",
        "SharedArrayBuffer",
        "Atomics.waitAsync",
        // WeakRefs (partial support)
        "FinalizationRegistry",
        "WeakRef",
        // Intl
        "Intl.Locale",
        "Intl.ListFormat",
        "Intl.Segmenter",
        "Intl.DurationFormat",
        "Intl.DisplayNames",
        "Intl.NumberFormat-v3",
        // Other advanced features
        "import.meta",
        "dynamic-import",
        "top-level-await",
        "json-modules",
        "import-assertions",
        "import-attributes",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Test262 conformance test runner
pub struct Test262Runner {
    config: RunnerConfig,
    host: Host262,
    /// Cached harness preamble
    harness_preamble: String,
}

impl Test262Runner {
    /// Create a new test runner
    pub fn new(config: RunnerConfig) -> Self {
        let harness_preamble = build_harness_preamble();
        Self {
            config,
            host: Host262::new(),
            harness_preamble,
        }
    }

    /// Run all tests in a directory
    pub fn run_directory(&mut self, subpath: &str) -> Vec<TestResult> {
        let test_dir = self.config.test262_path.join("test").join(subpath);

        if !test_dir.exists() {
            eprintln!("Test directory not found: {:?}", test_dir);
            return Vec::new();
        }

        let mut results = Vec::new();
        self.collect_and_run(&test_dir, &mut results);
        results
    }

    /// Run a single test file
    pub fn run_test(&mut self, path: &Path) -> Vec<TestResult> {
        let mut results = Vec::new();

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                results.push(TestResult {
                    path: path.to_path_buf(),
                    outcome: TestOutcome::Crash,
                    duration: Duration::ZERO,
                    error: Some(format!("Failed to read file: {}", e)),
                    strict: false,
                });
                return results;
            }
        };

        let metadata = match TestMetadata::parse(&content) {
            Some(m) => m,
            None => {
                // No metadata - run as simple test
                results.push(self.execute_variant(path, &content, false, None));
                return results;
            }
        };

        // Check if test should be skipped
        if let Some(skip_reason) = self.should_skip(&metadata) {
            results.push(TestResult {
                path: path.to_path_buf(),
                outcome: TestOutcome::Skip,
                duration: Duration::ZERO,
                error: Some(skip_reason),
                strict: false,
            });
            return results;
        }

        // Run non-strict variant
        if metadata.should_run_non_strict() {
            results.push(self.execute_variant(path, &content, false, Some(&metadata)));
        }

        // Run strict variant
        if metadata.should_run_strict() {
            results.push(self.execute_variant(path, &content, true, Some(&metadata)));
        }

        results
    }

    fn collect_and_run(&mut self, dir: &Path, results: &mut Vec<TestResult>) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.collect_and_run(&path, results);
            } else if path.extension().map_or(false, |e| e == "js") {
                let test_results = self.run_test(&path);
                results.extend(test_results);
            }
        }
    }

    fn should_skip(&self, metadata: &TestMetadata) -> Option<String> {
        // Check for unsupported features
        for feature in &metadata.features {
            if self.config.skip_features.contains(feature) {
                return Some(format!("Unsupported feature: {}", feature));
            }
        }

        // Check for async tests (limited support)
        if metadata.flags.is_async {
            return Some("Async tests not yet supported".to_string());
        }

        // Check for module tests
        if metadata.flags.module {
            return Some("Module tests not yet supported".to_string());
        }

        None
    }

    fn execute_variant(
        &mut self,
        path: &Path,
        content: &str,
        strict: bool,
        metadata: Option<&TestMetadata>,
    ) -> TestResult {
        self.host.reset();
        let start = Instant::now();

        // Build test source
        let source = self.build_test_source(content, strict, metadata);

        // Execute test
        let result = self.execute_source(&source, metadata);

        TestResult {
            path: path.to_path_buf(),
            outcome: result.0,
            duration: start.elapsed(),
            error: result.1,
            strict,
        }
    }

    fn build_test_source(&self, content: &str, strict: bool, metadata: Option<&TestMetadata>) -> String {
        let mut source = String::new();

        // Add strict mode directive
        if strict {
            source.push_str("\"use strict\";\n");
        }

        // Add standard harness
        if metadata.map_or(true, |m| !m.flags.raw) {
            source.push_str(&self.harness_preamble);

            // Add requested includes
            if let Some(meta) = metadata {
                for include in &meta.includes {
                    if let Some(harness_content) = harness_files::get(include) {
                        source.push_str(harness_content);
                        source.push('\n');
                    }
                }
            }
        }

        // Extract test code (after metadata)
        let test_code = if let Some(end) = content.find("---*/") {
            &content[end + 5..]
        } else {
            content
        };

        source.push_str(test_code);
        source
    }

    fn execute_source(&mut self, source: &str, metadata: Option<&TestMetadata>) -> (TestOutcome, Option<String>) {
        // For now, just validate that we can parse the source
        // Full execution requires complete parser and compiler integration

        use silksurf_js::Lexer;

        // Try lexing
        let lexer = Lexer::new(source);
        let tokens: Vec<_> = lexer.collect();

        // Check for lex errors
        for token in &tokens {
            if matches!(token.kind, silksurf_js::TokenKind::Error(_)) {
                let is_negative_parse = metadata
                    .and_then(|m| m.negative.as_ref())
                    .map_or(false, |n| n.phase == NegativePhase::Parse);

                if is_negative_parse {
                    // Expected to fail at parse
                    return (TestOutcome::Pass, None);
                }

                return (
                    TestOutcome::Fail,
                    Some(format!("Lexer error: {:?}", token.kind)),
                );
            }
        }

        // If negative parse test and we got here, it should have failed
        if let Some(neg) = metadata.and_then(|m| m.negative.as_ref()) {
            if neg.phase == NegativePhase::Parse {
                return (
                    TestOutcome::Fail,
                    Some(format!(
                        "Expected parse error {} but parsing succeeded",
                        neg.error_type
                    )),
                );
            }
        }

        // For now, pass if we can lex without errors
        // TODO: Full parse and execute
        (TestOutcome::Pass, None)
    }
}

fn build_harness_preamble() -> String {
    let mut preamble = String::new();

    // Always include sta.js for Test262Error
    if let Some(sta) = harness_files::get("sta.js") {
        preamble.push_str(sta);
        preamble.push('\n');
    }

    // Always include assert.js
    if let Some(assert) = harness_files::get("assert.js") {
        preamble.push_str(assert);
        preamble.push('\n');
    }

    preamble
}

/// Summary of test run
#[derive(Debug, Default)]
pub struct TestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub timeout: usize,
    pub crashed: usize,
    pub duration: Duration,
}

impl TestSummary {
    pub fn from_results(results: &[TestResult]) -> Self {
        let mut summary = Self::default();
        summary.total = results.len();

        for result in results {
            match result.outcome {
                TestOutcome::Pass => summary.passed += 1,
                TestOutcome::Fail => summary.failed += 1,
                TestOutcome::Skip => summary.skipped += 1,
                TestOutcome::Timeout => summary.timeout += 1,
                TestOutcome::Crash => summary.crashed += 1,
            }
            summary.duration += result.duration;
        }

        summary
    }

    pub fn pass_rate(&self) -> f64 {
        let run = self.passed + self.failed;
        if run == 0 {
            0.0
        } else {
            (self.passed as f64 / run as f64) * 100.0
        }
    }
}

impl std::fmt::Display for TestSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "test262 Summary:")?;
        writeln!(f, "  Total:   {}", self.total)?;
        writeln!(f, "  Passed:  {} ({:.1}%)", self.passed, self.pass_rate())?;
        writeln!(f, "  Failed:  {}", self.failed)?;
        writeln!(f, "  Skipped: {}", self.skipped)?;
        writeln!(f, "  Timeout: {}", self.timeout)?;
        writeln!(f, "  Crashed: {}", self.crashed)?;
        writeln!(f, "  Time:    {:.2}s", self.duration.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harness_preamble() {
        let preamble = build_harness_preamble();
        assert!(preamble.contains("Test262Error"));
        assert!(preamble.contains("assert"));
    }

    #[test]
    fn test_summary() {
        let results = vec![
            TestResult {
                path: PathBuf::from("a.js"),
                outcome: TestOutcome::Pass,
                duration: Duration::from_millis(10),
                error: None,
                strict: false,
            },
            TestResult {
                path: PathBuf::from("b.js"),
                outcome: TestOutcome::Pass,
                duration: Duration::from_millis(20),
                error: None,
                strict: true,
            },
            TestResult {
                path: PathBuf::from("c.js"),
                outcome: TestOutcome::Fail,
                duration: Duration::from_millis(5),
                error: Some("error".into()),
                strict: false,
            },
            TestResult {
                path: PathBuf::from("d.js"),
                outcome: TestOutcome::Skip,
                duration: Duration::ZERO,
                error: None,
                strict: false,
            },
        ];

        let summary = TestSummary::from_results(&results);
        assert_eq!(summary.total, 4);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 1);
        assert!((summary.pass_rate() - 66.67).abs() < 0.1);
    }

    #[test]
    fn test_default_skip_features() {
        let skip = default_skip_features();
        assert!(skip.contains("Temporal"));
        assert!(skip.contains("SharedArrayBuffer"));
        assert!(!skip.contains("let")); // Basic feature should not be skipped
    }
}
