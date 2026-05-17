use std::env;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use silksurf_css::parse_stylesheet_bytes;

const DEFAULT_EXPECTATIONS_FILE: &str = "silksurf-css-harness.expectations";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedOutcome {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Default)]
struct ExpectationConfig {
    expected_pass: Vec<String>,
    expected_fail: Vec<String>,
    skip: Vec<String>,
    source: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct HarnessOptions {
    include: Option<String>,
    exclude: Option<String>,
    max_files: Option<usize>,
    fail_on_xpass: bool,
}

impl HarnessOptions {
    fn load() -> Result<Self, String> {
        Ok(Self {
            include: optional_env_pattern("CSS_TEST_INCLUDE"),
            exclude: optional_env_pattern("CSS_TEST_EXCLUDE"),
            max_files: env_var_usize("CSS_TEST_MAX_FILES")?,
            fail_on_xpass: env_var_truthy("CSS_HARNESS_FAIL_ON_XPASS"),
        })
    }
}

impl ExpectationConfig {
    fn load(root: &Path) -> Result<Self, String> {
        let manifest = env::var("CSS_TEST_EXPECTATIONS").map_or_else(|_| root.join(DEFAULT_EXPECTATIONS_FILE), PathBuf::from);
        if !manifest.exists() {
            return Ok(Self::default());
        }

        let raw = fs::read_to_string(&manifest)
            .map_err(|error| format!("failed to read {}: {error}", manifest.display()))?;
        let mut config = Self::parse(&raw, &manifest)?;
        config.source = Some(manifest);
        Ok(config)
    }

    fn parse(raw: &str, source: &Path) -> Result<Self, String> {
        let mut config = Self::default();

        for (line_idx, line) in raw.lines().enumerate() {
            let without_comment = line.split('#').next().unwrap_or_default().trim();
            if without_comment.is_empty() {
                continue;
            }

            let mut parts = without_comment.split_whitespace();
            let directive = parts.next().ok_or_else(|| {
                format!("{}:{} missing directive", source.display(), line_idx + 1)
            })?;
            let pattern = parts
                .next()
                .ok_or_else(|| {
                    format!(
                        "{}:{} missing pattern after directive `{directive}`",
                        source.display(),
                        line_idx + 1
                    )
                })?
                .replace('\\', "/");

            if parts.next().is_some() {
                return Err(format!(
                    "{}:{} expected `directive pattern` format",
                    source.display(),
                    line_idx + 1
                ));
            }

            match directive {
                "expected-pass" => config.expected_pass.push(pattern),
                "expected-fail" => config.expected_fail.push(pattern),
                "skip" => config.skip.push(pattern),
                _ => {
                    return Err(format!(
                        "{}:{} unknown directive `{directive}` (expected-pass | expected-fail | skip)",
                        source.display(),
                        line_idx + 1
                    ));
                }
            }
        }

        Ok(config)
    }

    fn classify(&self, relative_path: &str) -> ExpectedOutcome {
        if self
            .skip
            .iter()
            .any(|pattern| wildcard_match(pattern, relative_path))
        {
            return ExpectedOutcome::Skip;
        }

        if self
            .expected_pass
            .iter()
            .any(|pattern| wildcard_match(pattern, relative_path))
        {
            return ExpectedOutcome::Pass;
        }

        if self
            .expected_fail
            .iter()
            .any(|pattern| wildcard_match(pattern, relative_path))
        {
            return ExpectedOutcome::Fail;
        }

        classify_by_convention(relative_path)
    }
}

#[derive(Debug)]
enum ParseOutcome {
    Passed,
    Failed(String),
    Panicked,
}

#[derive(Debug)]
struct CaseFailure {
    path: String,
    message: String,
}

#[derive(Default)]
struct HarnessSummary {
    total: usize,
    skipped: usize,
    expected_pass: usize,
    expected_fail: usize,
    passed: usize,
    xfailed: usize,
    xpassed: Vec<String>,
    failures: Vec<CaseFailure>,
    harness_failures: Vec<CaseFailure>,
}

impl HarnessSummary {
    fn new(total: usize) -> Self {
        Self {
            total,
            ..Self::default()
        }
    }

    fn record_harness_failure(&mut self, path: String, message: String) {
        self.harness_failures.push(CaseFailure { path, message });
    }

    fn record_case(&mut self, path: String, expected: ExpectedOutcome, outcome: ParseOutcome) {
        match expected {
            ExpectedOutcome::Skip => {
                self.skipped += 1;
            }
            ExpectedOutcome::Pass => {
                self.expected_pass += 1;
                match outcome {
                    ParseOutcome::Passed => self.passed += 1,
                    ParseOutcome::Failed(message) => {
                        self.failures.push(CaseFailure { path, message });
                    }
                    ParseOutcome::Panicked => {
                        self.failures.push(CaseFailure {
                            path,
                            message: "parser panicked".to_string(),
                        });
                    }
                }
            }
            ExpectedOutcome::Fail => {
                self.expected_fail += 1;
                match outcome {
                    ParseOutcome::Passed => self.xpassed.push(path),
                    ParseOutcome::Failed(_) | ParseOutcome::Panicked => self.xfailed += 1,
                }
            }
        }
    }

    fn has_hard_failures(&self, fail_on_xpass: bool) -> bool {
        !self.failures.is_empty()
            || !self.harness_failures.is_empty()
            || (fail_on_xpass && !self.xpassed.is_empty())
    }

    fn print(&self, root: &Path, expectations: &ExpectationConfig, options: &HarnessOptions) {
        let fail_on_xpass = options.fail_on_xpass;
        let executed = self.total.saturating_sub(self.skipped);
        eprintln!(
            "[css-harness] root={} total={} executed={} expected-pass={} expected-fail={} skipped={} passed={} xfailed={} xpassed={} failures={} harness-failures={}",
            root.display(),
            self.total,
            executed,
            self.expected_pass,
            self.expected_fail,
            self.skipped,
            self.passed,
            self.xfailed,
            self.xpassed.len(),
            self.failures.len(),
            self.harness_failures.len(),
        );

        if let Some(source) = &expectations.source {
            eprintln!("[css-harness] expectations={}", source.display());
        } else {
            eprintln!(
                "[css-harness] expectations=conventions-only (set CSS_TEST_EXPECTATIONS or {DEFAULT_EXPECTATIONS_FILE} for overrides)"
            );
        }

        if let Some(include) = &options.include {
            eprintln!("[css-harness] include={include}");
        }
        if let Some(exclude) = &options.exclude {
            eprintln!("[css-harness] exclude={exclude}");
        }
        if let Some(max_files) = options.max_files {
            eprintln!("[css-harness] max-files={max_files}");
        }

        for failure in &self.harness_failures {
            eprintln!(
                "[css-harness] harness-failure {} :: {}",
                failure.path, failure.message
            );
        }

        for failure in &self.failures {
            eprintln!(
                "[css-harness] unexpected-failure {} :: {}",
                failure.path, failure.message
            );
        }

        if !self.xpassed.is_empty() {
            let mode = if fail_on_xpass {
                "fail"
            } else {
                "report-only (set CSS_HARNESS_FAIL_ON_XPASS=1 to fail)"
            };
            eprintln!(
                "[css-harness] unexpected-pass mode: {mode}; {} case(s)",
                self.xpassed.len()
            );
            for path in &self.xpassed {
                eprintln!("[css-harness] unexpected-pass {path}");
            }
        }
    }
}

#[test]
fn css_harness_compliance() {
    let Ok(base) = env::var("CSS_TESTS_DIR") else {
        eprintln!("[css-harness] skipped: CSS_TESTS_DIR not set");
        return;
    };

    let root = PathBuf::from(base);
    assert!(
        root.is_dir(),
        "[css-harness] CSS_TESTS_DIR must point to a directory: {}",
        root.display()
    );

    let expectations = ExpectationConfig::load(&root)
        .unwrap_or_else(|error| panic!("[css-harness] failed to load expectations: {error}"));
    let options = HarnessOptions::load()
        .unwrap_or_else(|error| panic!("[css-harness] failed to parse harness options: {error}"));

    let mut files = Vec::new();
    collect_css_files(&root, &mut files)
        .unwrap_or_else(|error| panic!("[css-harness] failed to scan {}: {error}", root.display()));

    files.sort();
    let files = apply_file_filters(files, &root, &options);
    assert!(
        !files.is_empty(),
        "[css-harness] no .css files selected under {} (check CSS_TEST_INCLUDE/CSS_TEST_EXCLUDE/CSS_TEST_MAX_FILES)",
        root.display()
    );

    let mut summary = HarnessSummary::new(files.len());
    for (file, relative) in files {
        let expectation = expectations.classify(&relative);
        if expectation == ExpectedOutcome::Skip {
            summary.record_case(relative, expectation, ParseOutcome::Passed);
            continue;
        }

        let data = match fs::read(&file) {
            Ok(data) => data,
            Err(error) => {
                summary.record_harness_failure(relative, format!("failed to read file: {error}"));
                continue;
            }
        };

        let outcome = parse_case(&data);
        summary.record_case(relative, expectation, outcome);
    }

    summary.print(&root, &expectations, &options);

    assert!(
        !summary.has_hard_failures(options.fail_on_xpass),
        "[css-harness] compliance run reported failures"
    );
}

fn parse_case(input: &[u8]) -> ParseOutcome {
    match panic::catch_unwind(AssertUnwindSafe(|| parse_stylesheet_bytes(input))) {
        Ok(Ok(_)) => ParseOutcome::Passed,
        Ok(Err(error)) => ParseOutcome::Failed(format!(
            "parse error at offset {}: {}",
            error.offset, error.message
        )),
        Err(_) => ParseOutcome::Panicked,
    }
}

fn apply_file_filters(
    files: Vec<PathBuf>,
    root: &Path,
    options: &HarnessOptions,
) -> Vec<(PathBuf, String)> {
    let mut selected = Vec::new();
    for file in files {
        let relative = normalized_relative_path(&file, root);
        if let Some(include) = options.include.as_deref()
            && !wildcard_match(include, &relative)
        {
            continue;
        }
        if let Some(exclude) = options.exclude.as_deref()
            && wildcard_match(exclude, &relative)
        {
            continue;
        }
        selected.push((file, relative));
    }
    if let Some(max_files) = options.max_files {
        selected.truncate(max_files);
    }
    selected
}

fn collect_css_files(root: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::path);

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_css_files(&path, files)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("css"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn normalized_relative_path(file: &Path, root: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    relative.to_string_lossy().replace('\\', "/")
}

fn classify_by_convention(relative_path: &str) -> ExpectedOutcome {
    let lower = relative_path.to_ascii_lowercase();

    if lower
        .split('/')
        .any(|component| component == "support" || component == "resources")
    {
        return ExpectedOutcome::Skip;
    }

    let file_name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if lower.split('/').any(|component| component == "invalid")
        || file_name.contains(".invalid.")
        || file_name.ends_with("-invalid.css")
        || file_name.ends_with("_invalid.css")
    {
        return ExpectedOutcome::Fail;
    }

    ExpectedOutcome::Pass
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();

    let mut pattern_idx = 0usize;
    let mut value_idx = 0usize;
    let mut star_idx = None;
    let mut star_value_idx = 0usize;

    while value_idx < value.len() {
        if pattern_idx < pattern.len()
            && (pattern[pattern_idx] == b'?' || pattern[pattern_idx] == value[value_idx])
        {
            pattern_idx += 1;
            value_idx += 1;
            continue;
        }

        if pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
            star_idx = Some(pattern_idx);
            pattern_idx += 1;
            star_value_idx = value_idx;
            continue;
        }

        if let Some(previous_star) = star_idx {
            pattern_idx = previous_star + 1;
            star_value_idx += 1;
            value_idx = star_value_idx;
            continue;
        }

        return false;
    }

    while pattern_idx < pattern.len() && pattern[pattern_idx] == b'*' {
        pattern_idx += 1;
    }

    pattern_idx == pattern.len()
}

fn env_var_truthy(name: &str) -> bool {
    match env::var(name) {
        Ok(value) => {
            value == "1"
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
                || value.eq_ignore_ascii_case("on")
        }
        Err(_) => false,
    }
}

fn optional_env_pattern(name: &str) -> Option<String> {
    let value = env::var(name).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.replace('\\', "/"))
    }
}

fn env_var_usize(name: &str) -> Result<Option<usize>, String> {
    match env::var(name) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let parsed = trimmed
                .parse::<usize>()
                .map_err(|_| format!("{name} must be a positive integer (got `{trimmed}`)"))?;
            if parsed == 0 {
                return Err(format!("{name} must be greater than zero"));
            }
            Ok(Some(parsed))
        }
        Err(_) => Ok(None),
    }
}

#[test]
fn wildcard_match_handles_common_patterns() {
    assert!(wildcard_match("invalid/*", "invalid/test.css"));
    assert!(wildcard_match("invalid/*", "invalid/deeper/test.css"));
    assert!(wildcard_match("*.css", "foo/bar/test.css"));
    assert!(wildcard_match("foo/???.css", "foo/abc.css"));
    assert!(!wildcard_match("foo/???.css", "foo/abcd.css"));
}

#[test]
fn expectations_override_conventions() {
    let config = ExpectationConfig::parse(
        r"
        expected-fail invalid/*
        expected-pass invalid/forced-pass.css
        skip support/*
        ",
        Path::new("inline.expectations"),
    )
    .expect("parse expectations");

    assert_eq!(
        config.classify("invalid/forced-pass.css"),
        ExpectedOutcome::Pass
    );
    assert_eq!(config.classify("invalid/broken.css"), ExpectedOutcome::Fail);
    assert_eq!(config.classify("support/helper.css"), ExpectedOutcome::Skip);
    assert_eq!(config.classify("valid/base.css"), ExpectedOutcome::Pass);
}

#[test]
fn file_filters_apply_include_exclude_and_limit() {
    let root = PathBuf::from("/tmp/css-harness-root");
    let files = vec![
        root.join("invalid/a.css"),
        root.join("invalid/forced-pass.css"),
        root.join("valid/base.css"),
    ];
    let options = HarnessOptions {
        include: Some("invalid/*".to_string()),
        exclude: Some("*forced*".to_string()),
        max_files: Some(1),
        fail_on_xpass: false,
    };

    let selected = apply_file_filters(files, &root, &options);
    let selected_paths: Vec<String> = selected
        .into_iter()
        .map(|(_, relative_path)| relative_path)
        .collect();

    assert_eq!(selected_paths, vec!["invalid/a.css"]);
}
