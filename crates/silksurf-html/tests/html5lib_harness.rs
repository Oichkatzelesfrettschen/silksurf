//! html5lib tokenizer conformance for the auxiliary `silksurf_html::Tokenizer`.
//!
//! This measures the hand-written tokenizer, which serves tooling rather than
//! page loads: `silksurf_engine::bin::wpt_runner` and the `silksurf-css`
//! harness both use it to lift `<style>` contents out of source markup.
//! Production HTML tree construction runs through `silksurf_html::parse_html`
//! on html5ever, and `html5lib_tree_construction.rs` measures that path.
//!
//! Every case in the corpus runs. Known failures are recorded as
//! `expected-fail` directives in the expectations file rather than filtered
//! from the run, so a regression and a repair are both a diff against that
//! file. A case identifier is `<file>:<index>`.
//!
//! `HTML5LIB_TESTS_DIR` selects the corpus, `HTML5LIB_SCORECARD` names a JSON
//! output path, and `HTML5LIB_FAIL_ON_XPASS` promotes an unexpected pass to a
//! hard failure.

use std::env;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use silksurf_html::{Attribute, Token, Tokenizer};

const DEFAULT_CORPUS_DIR: &str = "silksurf-extras/html5lib-tests/tokenizer";
const EXPECTATIONS_FILE: &str = "tests/html5lib-tokenizer.expectations";

// ---- corpus model -----------------------------------------------------------

struct TokenizerCase {
    id: String,
    description: String,
    input: String,
    output: Vec<Value>,
    /// Set when the case needs tokenizer state the public API cannot express.
    unsupported: Option<&'static str>,
}

/// Decode the `\uXXXX` escapes html5lib applies when `doubleEscaped` is set.
fn undouble_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        if chars.as_str().starts_with('u') {
            chars.next();
            let hex: String = chars.by_ref().take(4).collect();
            match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                Some(decoded) => out.push(decoded),
                // Lone surrogates are legal in this corpus and have no char
                // representation; U+FFFD keeps the comparison well-formed.
                None => out.push('\u{FFFD}'),
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn undouble_escape_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(undouble_escape(text)),
        Value::Array(items) => Value::Array(items.iter().map(undouble_escape_value).collect()),
        other => other.clone(),
    }
}

/// Read one `.test` file into cases.
///
/// `initialStates` beyond the data state and `lastStartTag` both require
/// tokenizer entry points `silksurf_html::Tokenizer` does not expose, so those
/// cases carry an `unsupported` reason and count as skipped rather than failed.
fn parse_test_file(stem: &str, raw: &str) -> Result<Vec<TokenizerCase>, String> {
    let root: Value =
        serde_json::from_str(raw).map_err(|error| format!("{stem}: invalid JSON: {error}"))?;
    let Some(tests) = root.get("tests").and_then(Value::as_array) else {
        // xmlViolation.test and README.md carry no `tests` array.
        return Ok(Vec::new());
    };

    let mut cases = Vec::new();
    for (index, case) in tests.iter().enumerate() {
        let description = case
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("(no description)")
            .to_string();
        let Some(input) = case.get("input").and_then(Value::as_str) else {
            return Err(format!("{stem}:{index} has no input"));
        };
        let Some(output) = case.get("output").and_then(Value::as_array) else {
            return Err(format!("{stem}:{index} has no output"));
        };

        let double_escaped = case
            .get("doubleEscaped")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (input, output) = if double_escaped {
            (
                undouble_escape(input),
                output.iter().map(undouble_escape_value).collect(),
            )
        } else {
            (input.to_string(), output.clone())
        };

        let non_data_state = case
            .get("initialStates")
            .and_then(Value::as_array)
            .is_some_and(|states| {
                states
                    .iter()
                    .any(|state| state.as_str() != Some("Data state"))
            });
        let unsupported = if non_data_state {
            Some("initialStates beyond the data state have no public entry point")
        } else if case.get("lastStartTag").is_some() {
            Some("lastStartTag has no public entry point")
        } else {
            None
        };

        cases.push(TokenizerCase {
            id: format!("{stem}:{index}"),
            description,
            input,
            output,
            unsupported,
        });
    }
    Ok(cases)
}

// ---- token conversion -------------------------------------------------------

fn tokenize_as_html5lib_values(input: &str) -> Result<Vec<Value>, silksurf_html::TokenizeError> {
    let mut tokenizer = Tokenizer::new();
    let mut tokens = tokenizer.feed(input)?;
    tokens.extend(tokenizer.finish()?);
    Ok(tokens
        .into_iter()
        .filter_map(token_as_html5lib_value)
        .collect())
}

fn token_as_html5lib_value(token: Token) -> Option<Value> {
    match token {
        // html5lib's fifth DOCTYPE field is `correctness`, which is true when
        // the doctype does NOT force quirks mode. Emitting force_quirks
        // directly inverts every DOCTYPE comparison.
        Token::Doctype {
            name,
            public_id,
            system_id,
            force_quirks,
        } => Some(json!([
            "DOCTYPE",
            string_or_null(name),
            string_or_null(public_id),
            string_or_null(system_id),
            !force_quirks
        ])),
        Token::StartTag {
            name,
            attributes,
            self_closing,
        } => Some(start_tag_as_html5lib_value(name, attributes, self_closing)),
        Token::EndTag { name } => Some(json!(["EndTag", name])),
        Token::Comment { data } => Some(json!(["Comment", data])),
        Token::Character { data } => Some(json!(["Character", data])),
        Token::Eof => None,
    }
}

fn start_tag_as_html5lib_value(
    name: String,
    attributes: Vec<Attribute>,
    self_closing: bool,
) -> Value {
    let mut attrs = Map::new();
    for attribute in attributes {
        attrs.insert(
            attribute.name,
            Value::String(attribute.value.unwrap_or_default()),
        );
    }

    let mut token = vec![
        Value::String("StartTag".to_string()),
        Value::String(name),
        Value::Object(attrs),
    ];
    if self_closing {
        token.push(Value::Bool(true));
    }
    Value::Array(token)
}

fn string_or_null(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

/// html5lib splits character data across tokens freely; the standard comparison
/// concatenates adjacent `Character` tokens before matching.
fn coalesce_characters(values: Vec<Value>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(values.len());
    for value in values {
        let is_character = value
            .get(0)
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "Character");
        if is_character
            && let Some(previous) = out.last_mut()
            && previous.get(0).and_then(Value::as_str) == Some("Character")
        {
            let addition = value.get(1).and_then(Value::as_str).unwrap_or_default();
            let merged = format!(
                "{}{addition}",
                previous.get(1).and_then(Value::as_str).unwrap_or_default()
            );
            *previous = json!(["Character", merged]);
            continue;
        }
        out.push(value);
    }
    out
}

// ---- expectations -----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expected {
    Pass,
    Fail,
    Skip,
}

#[derive(Default)]
struct Expectations {
    expected_fail: Vec<String>,
    skip: Vec<String>,
    source: Option<PathBuf>,
}

impl Expectations {
    fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut config = Self::parse(&raw, path)?;
        config.source = Some(path.to_path_buf());
        Ok(config)
    }

    fn parse(raw: &str, source: &Path) -> Result<Self, String> {
        let mut config = Self::default();
        for (index, line) in raw.lines().enumerate() {
            let text = line.split('#').next().unwrap_or_default().trim();
            if text.is_empty() {
                continue;
            }
            let mut parts = text.split_whitespace();
            let directive = parts.next().unwrap_or_default();
            let pattern = parts.next().ok_or_else(|| {
                format!(
                    "{}:{} missing pattern after `{directive}`",
                    source.display(),
                    index + 1
                )
            })?;
            match directive {
                "expected-fail" => config.expected_fail.push(pattern.to_string()),
                "skip" => config.skip.push(pattern.to_string()),
                _ => {
                    return Err(format!(
                        "{}:{} unknown directive `{directive}` (expected-fail | skip)",
                        source.display(),
                        index + 1
                    ));
                }
            }
        }
        Ok(config)
    }

    fn classify(&self, id: &str) -> Expected {
        if self.skip.iter().any(|p| wildcard_match(p, id)) {
            return Expected::Skip;
        }
        if self.expected_fail.iter().any(|p| wildcard_match(p, id)) {
            return Expected::Fail;
        }
        Expected::Pass
    }
}

// ---- runner -----------------------------------------------------------------

#[derive(Default)]
struct Summary {
    total: usize,
    skipped: usize,
    unsupported: usize,
    passed: usize,
    xfailed: usize,
    xpassed: Vec<String>,
    failures: Vec<(String, String)>,
}

#[test]
fn html5lib_tokenizer_conformance() {
    let repo_root = repo_root();
    let corpus = env::var("HTML5LIB_TESTS_DIR")
        .map_or_else(|_| repo_root.join(DEFAULT_CORPUS_DIR), PathBuf::from);

    // An absent corpus is a skip carrying its own remedy. An operator-supplied
    // path that does not resolve is a hard failure, because that is a broken
    // invocation rather than a missing optional input.
    if !corpus.is_dir() {
        assert!(
            env::var("HTML5LIB_TESTS_DIR").is_err(),
            "HTML5LIB_TESTS_DIR is set but not a directory: {}",
            corpus.display()
        );
        eprintln!(
            "[html5lib-tokenizer] skipped: corpus absent at {}; run scripts/fetch_html_css_test_corpora.sh",
            corpus.display()
        );
        return;
    }

    let expectations_path = repo_root
        .join("crates/silksurf-html")
        .join(EXPECTATIONS_FILE);
    let expectations = Expectations::load(&expectations_path)
        .unwrap_or_else(|error| panic!("[html5lib-tokenizer] {error}"));

    let mut test_files: Vec<PathBuf> = fs::read_dir(&corpus)
        .unwrap_or_else(|error| panic!("[html5lib-tokenizer] scan {}: {error}", corpus.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "test"))
        .collect();
    test_files.sort();

    assert!(
        !test_files.is_empty(),
        "[html5lib-tokenizer] no .test files under {}",
        corpus.display()
    );

    let mut summary = Summary::default();
    for file in &test_files {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let raw = fs::read_to_string(file).unwrap_or_else(|error| {
            panic!("[html5lib-tokenizer] read {}: {error}", file.display())
        });
        let cases = parse_test_file(stem, &raw)
            .unwrap_or_else(|error| panic!("[html5lib-tokenizer] {error}"));

        for case in cases {
            summary.total += 1;
            if case.unsupported.is_some() {
                summary.unsupported += 1;
                continue;
            }
            let expected = expectations.classify(&case.id);
            if expected == Expected::Skip {
                summary.skipped += 1;
                continue;
            }

            let outcome = run_case(&case);
            match (expected, &outcome) {
                (Expected::Pass, Ok(())) => summary.passed += 1,
                (Expected::Pass, Err(message)) => {
                    summary.failures.push((case.id, message.clone()));
                }
                (Expected::Fail, Ok(())) => summary.xpassed.push(case.id),
                (Expected::Fail, Err(_)) => summary.xfailed += 1,
                (Expected::Skip, _) => unreachable!("skip returns above"),
            }
        }
    }

    report(&summary, &corpus, &expectations);
    write_scorecard(&summary);

    let fail_on_xpass = env::var("HTML5LIB_FAIL_ON_XPASS").is_ok_and(|value| value == "1");
    assert!(
        summary.failures.is_empty(),
        "[html5lib-tokenizer] {} unexpected failure(s); record them as expected-fail in {} or fix the tokenizer",
        summary.failures.len(),
        expectations_path.display()
    );
    assert!(
        !fail_on_xpass || summary.xpassed.is_empty(),
        "[html5lib-tokenizer] {} case(s) now pass; drop their expected-fail lines",
        summary.xpassed.len()
    );
}

fn run_case(case: &TokenizerCase) -> Result<(), String> {
    let produced = panic::catch_unwind(AssertUnwindSafe(|| {
        tokenize_as_html5lib_values(&case.input)
    }))
    .map_err(|_| "tokenizer panicked".to_string())?;

    let actual = match produced {
        Ok(values) => coalesce_characters(values),
        Err(error) => {
            return Err(format!(
                "{}: tokenizer error at offset {}: {}",
                case.description, error.offset, error.message
            ));
        }
    };
    let expected = coalesce_characters(case.output.clone());
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "{}: input {:?} expected {} got {}",
        case.description,
        case.input,
        Value::Array(expected),
        Value::Array(actual)
    ))
}

fn report(summary: &Summary, corpus: &Path, expectations: &Expectations) {
    let executed = summary.total - summary.skipped - summary.unsupported;
    eprintln!(
        "[html5lib-tokenizer] corpus={} total={} executed={} passed={} xfailed={} xpassed={} failures={} skipped={} unsupported={}",
        corpus.display(),
        summary.total,
        executed,
        summary.passed,
        summary.xfailed,
        summary.xpassed.len(),
        summary.failures.len(),
        summary.skipped,
        summary.unsupported,
    );
    // Conformance counts genuine passes alone; folding xfailed into the
    // numerator would restate a recorded gap as a case the tokenizer handles.
    eprintln!(
        "[html5lib-tokenizer] conformance={:.2}% of executed, {:.2}% of total",
        ratio(summary.passed, executed) * 100.0,
        ratio(summary.passed, summary.total) * 100.0
    );
    eprintln!(
        "[html5lib-tokenizer] gate={} ({} unexpected failure(s), {} recorded gap(s))",
        if summary.failures.is_empty() {
            "green"
        } else {
            "red"
        },
        summary.failures.len(),
        summary.xfailed,
    );
    match &expectations.source {
        Some(path) => eprintln!("[html5lib-tokenizer] expectations={}", path.display()),
        None => eprintln!("[html5lib-tokenizer] expectations=none (every case must pass)"),
    }
    for (id, message) in &summary.failures {
        eprintln!("[html5lib-tokenizer] unexpected-failure {id} :: {message}");
    }
    for id in &summary.xpassed {
        eprintln!("[html5lib-tokenizer] unexpected-pass {id}");
    }
}

fn write_scorecard(summary: &Summary) {
    let Ok(raw_path) = env::var("HTML5LIB_SCORECARD") else {
        return;
    };
    let requested = PathBuf::from(&raw_path);
    let path = if requested.is_absolute() {
        requested
    } else {
        repo_root().join(requested)
    };
    let executed = summary.total - summary.skipped - summary.unsupported;
    let revision = corpus_revision().unwrap_or_else(|| "unknown".to_string());
    let json = format!(
        "{{\n  \"runner\": \"html5lib_tokenizer\",\n  \"runner_kind\": \"html5lib-tokenizer\",\n  \"corpus\": \"html5lib-tests tokenizer\",\n  \"corpus_revision\": \"{revision}\",\n  \"oracle\": \"the token stream Tokenizer emits equals the corpus #output after html5lib normalization; a case whose id the expectations file marks expected-fail counts as a recorded gap rather than a pass\",\n  \"total\": {},\n  \"executed\": {},\n  \"pass\": {},\n  \"expected_fail\": {},\n  \"skip\": {},\n  \"unsupported\": {},\n  \"rate_executed\": {:.4},\n  \"rate_total\": {:.4}\n}}\n",
        summary.total,
        executed,
        summary.passed,
        summary.xfailed,
        summary.skipped,
        summary.unsupported,
        ratio(summary.passed, executed),
        ratio(summary.passed, summary.total),
    );
    if let Err(error) = fs::write(&path, json) {
        eprintln!(
            "[html5lib-tokenizer] scorecard write failed for {}: {error}",
            path.display()
        );
    }
}

fn corpus_revision() -> Option<String> {
    let manifest = repo_root().join("silksurf-extras/html-css-test-corpora-revisions.txt");
    let raw = fs::read_to_string(manifest).ok()?;
    raw.lines()
        .find(|line| line.starts_with("html5lib-tests "))
        .and_then(|line| line.split_whitespace().nth(1))
        .map(str::to_string)
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "corpus counts stay far below f64 integer precision"
    )]
    {
        numerator as f64 / denominator as f64
    }
}

/// CARGO_MANIFEST_DIR points at crates/silksurf-html; the repository root is
/// two levels above it.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut p = 0usize;
    let mut v = 0usize;
    let mut star = None;
    let mut star_v = 0usize;

    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            star_v = v;
            p += 1;
        } else if let Some(previous) = star {
            p = previous + 1;
            star_v += 1;
            v = star_v;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

// ---- harness self-tests -----------------------------------------------------

#[test]
fn double_escape_decodes_unicode_escapes() {
    assert_eq!(undouble_escape(r"abc"), "abc");
    assert_eq!(undouble_escape(r"\uD800"), "\u{FFFD}");
    assert_eq!(undouble_escape("plain"), "plain");
}

#[test]
fn adjacent_character_tokens_coalesce() {
    let merged = coalesce_characters(vec![
        json!(["Character", "ab"]),
        json!(["Character", "cd"]),
        json!(["EndTag", "p"]),
        json!(["Character", "e"]),
    ]);
    assert_eq!(
        merged,
        vec![
            json!(["Character", "abcd"]),
            json!(["EndTag", "p"]),
            json!(["Character", "e"]),
        ]
    );
}

#[test]
fn unsupported_cases_carry_a_reason() {
    let raw = r#"{"tests":[
        {"description":"script state","input":"x","output":[],"initialStates":["Script data state"]},
        {"description":"last tag","input":"x","output":[],"lastStartTag":"script"},
        {"description":"plain","input":"x","output":[]}
    ]}"#;
    let cases = parse_test_file("sample", raw).expect("parse");
    assert!(cases[0].unsupported.is_some());
    assert!(cases[1].unsupported.is_some());
    assert!(cases[2].unsupported.is_none());
}

#[test]
fn expectations_classify_by_case_id() {
    let config = Expectations::parse(
        "expected-fail namedEntities:*\nskip test3:12\n",
        Path::new("inline"),
    )
    .expect("parse");
    assert_eq!(config.classify("namedEntities:99"), Expected::Fail);
    assert_eq!(config.classify("test3:12"), Expected::Skip);
    assert_eq!(config.classify("test1:0"), Expected::Pass);
}
