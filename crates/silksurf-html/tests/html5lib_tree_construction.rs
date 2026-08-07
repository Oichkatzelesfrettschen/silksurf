//! WHATWG tree-construction conformance over the production parse path.
//!
//! html5lib retired its own `tree-construction/` directory; the corpus now
//! lives in WPT under `html/syntax/parsing/resources/` as `.dat` files. Each
//! case pairs a `#data` input with the `#document` tree the HTML standard
//! requires. This harness feeds `#data` through `silksurf_html::parse_html` --
//! the same entry point `silksurf_engine` uses at `lib.rs` -- and compares a
//! html5lib-format serialization of the resulting `Dom` against `#document`.
//!
//! Known failures are recorded as `expected-fail` directives in the
//! expectations file rather than filtered out of the corpus, so both a
//! regression and a repair surface as a diff against that file. A case
//! identifier is `<file>:<index>`, which the wildcard matcher accepts.
//!
//! `WPT_HTML_PARSING_DIR` selects the corpus; `HTML5LIB_TREE_SCORECARD` names
//! a JSON output path; `HTML5LIB_TREE_FAIL_ON_XPASS` promotes an unexpected
//! pass to a hard failure.

use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use silksurf_dom::{Dom, Namespace, NodeId, NodeKind};
use silksurf_html::parse_html;

const DEFAULT_CORPUS_DIR: &str =
    "silksurf-extras/wpt-css-parser-subset/html/syntax/parsing/resources";
const EXPECTATIONS_FILE: &str = "tests/html5lib-tree-construction.expectations";

// ---- corpus model -----------------------------------------------------------

/// One `#data` / `#document` pair from a `.dat` file.
struct TreeCase {
    id: String,
    data: String,
    document: String,
    fragment_context: Option<String>,
}

/// Split a `.dat` file into cases.
///
/// Section headers are recognized only when the whole line matches, because
/// `#data` payloads legitimately contain lines starting with `#`. The
/// `#document` section runs to the blank line that terminates the case.
fn parse_dat_file(file_stem: &str, raw: &str) -> Vec<TreeCase> {
    let mut cases = Vec::new();
    let mut lines = raw.lines().peekable();
    let mut index = 0usize;

    while let Some(line) = lines.next() {
        if line != "#data" {
            continue;
        }

        let mut data = Vec::new();
        let mut document = Vec::new();
        let mut fragment_context = None;
        let mut section = "data";

        for body in lines.by_ref() {
            match body {
                "#errors" | "#new-errors" | "#script-on" | "#script-off" => {
                    section = "ignored";
                    continue;
                }
                "#document-fragment" => {
                    section = "fragment";
                    continue;
                }
                "#document" => {
                    section = "document";
                    continue;
                }
                _ => {}
            }

            match section {
                "data" => data.push(body),
                "fragment" => fragment_context = Some(body.trim().to_string()),
                "document" => {
                    // A blank line closes the case; #document is always last.
                    if body.is_empty() {
                        break;
                    }
                    document.push(body);
                }
                _ => {}
            }
        }

        cases.push(TreeCase {
            id: format!("{file_stem}:{index}"),
            data: data.join("\n"),
            document: document.join("\n"),
            fragment_context,
        });
        index += 1;
    }

    cases
}

// ---- html5lib serialization -------------------------------------------------

/// Render `dom` in the html5lib `#document` format.
///
/// Each line is `| ` followed by two spaces per depth level. Elements outside
/// the HTML namespace carry a `svg ` or `math ` prefix, and attributes sort by
/// name on their own line one level below their element, both per the format
/// html5lib-tests documents.
fn serialize_dom(dom: &Dom) -> String {
    let mut out = String::new();
    let root = NodeId::from_raw(0);
    if let Ok(children) = dom.children(root) {
        for &child in children {
            serialize_node(dom, child, 0, &mut out);
        }
    }
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

fn serialize_node(dom: &Dom, id: NodeId, depth: usize, out: &mut String) {
    let Ok(node) = dom.node(id) else {
        return;
    };
    let indent = "  ".repeat(depth);

    match node.kind() {
        NodeKind::Document => {}
        NodeKind::Doctype {
            name,
            public_id,
            system_id,
        } => {
            let name = name.as_deref().unwrap_or("");
            match (public_id.as_deref(), system_id.as_deref()) {
                (None, None) => {
                    let _ = writeln!(out, "| {indent}<!DOCTYPE {name}>");
                }
                (public_id, system_id) => {
                    let _ = writeln!(
                        out,
                        "| {indent}<!DOCTYPE {name} \"{}\" \"{}\">",
                        public_id.unwrap_or(""),
                        system_id.unwrap_or("")
                    );
                }
            }
        }
        NodeKind::Text { text } => {
            let _ = writeln!(out, "| {indent}\"{text}\"");
        }
        NodeKind::Comment { data } => {
            let _ = writeln!(out, "| {indent}<!-- {data} -->");
        }
        NodeKind::Element {
            namespace,
            attributes,
            ..
        } => {
            let local = dom.element_name(id).ok().flatten().unwrap_or_default();
            let prefix = match namespace {
                Namespace::Html => "",
                Namespace::Svg => "svg ",
                Namespace::MathMl => "math ",
                Namespace::Other(_) => "",
            };
            let _ = writeln!(out, "| {indent}<{prefix}{local}>");

            // BTreeMap orders attributes by name, which the format requires.
            let sorted: BTreeMap<&str, &str> = attributes
                .iter()
                .map(|attr| (attr.name.as_str(), attr.value.as_str()))
                .collect();
            let attr_indent = "  ".repeat(depth + 1);
            for (name, value) in sorted {
                let _ = writeln!(out, "| {attr_indent}{name}=\"{value}\"");
            }
        }
    }

    if let Ok(children) = dom.children(id) {
        for &child in children {
            serialize_node(dom, child, depth + 1, out);
        }
    }
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
    passed: usize,
    xfailed: usize,
    xpassed: Vec<String>,
    failures: Vec<(String, String)>,
}

#[test]
fn html5lib_tree_construction_conformance() {
    let repo_root = repo_root();
    let corpus = env::var("WPT_HTML_PARSING_DIR")
        .map_or_else(|_| repo_root.join(DEFAULT_CORPUS_DIR), PathBuf::from);

    // An absent corpus is a skip with a reason, never a silent pass. An
    // operator-supplied path that does not resolve is a hard failure, because
    // that is a broken invocation rather than a missing optional input.
    if !corpus.is_dir() {
        assert!(
            env::var("WPT_HTML_PARSING_DIR").is_err(),
            "WPT_HTML_PARSING_DIR is set but not a directory: {}",
            corpus.display()
        );
        eprintln!(
            "[tree-construction] skipped: corpus absent at {}; run scripts/fetch_html_css_test_corpora.sh",
            corpus.display()
        );
        return;
    }

    let expectations_path = repo_root
        .join("crates/silksurf-html")
        .join(EXPECTATIONS_FILE);
    let expectations = Expectations::load(&expectations_path)
        .unwrap_or_else(|error| panic!("[tree-construction] {error}"));

    let mut dat_files: Vec<PathBuf> = fs::read_dir(&corpus)
        .unwrap_or_else(|error| panic!("[tree-construction] scan {}: {error}", corpus.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "dat"))
        .collect();
    dat_files.sort();

    assert!(
        !dat_files.is_empty(),
        "[tree-construction] no .dat files under {}",
        corpus.display()
    );

    let mut summary = Summary::default();
    for file in &dat_files {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let raw = fs::read_to_string(file)
            .unwrap_or_else(|error| panic!("[tree-construction] read {}: {error}", file.display()));

        for case in parse_dat_file(stem, &raw) {
            summary.total += 1;
            let expected = expectations.classify(&case.id);
            if expected == Expected::Skip {
                summary.skipped += 1;
                continue;
            }

            // Fragment cases need an innerHTML-mode entry point that takes a
            // context element; parse_html is document-mode only. They count as
            // skipped rather than failed so the rate reflects what ran.
            if case.fragment_context.is_some() {
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

    let fail_on_xpass = env::var("HTML5LIB_TREE_FAIL_ON_XPASS").is_ok_and(|v| v == "1");
    assert!(
        summary.failures.is_empty(),
        "[tree-construction] {} unexpected failure(s); record them as expected-fail in {} or fix the parser",
        summary.failures.len(),
        expectations_path.display()
    );
    assert!(
        !fail_on_xpass || summary.xpassed.is_empty(),
        "[tree-construction] {} case(s) now pass; drop their expected-fail lines",
        summary.xpassed.len()
    );
}

fn run_case(case: &TreeCase) -> Result<(), String> {
    let parsed = panic::catch_unwind(AssertUnwindSafe(|| serialize_dom(&parse_html(&case.data))));
    let actual = parsed.map_err(|_| "parser panicked".to_string())?;
    if actual == case.document {
        return Ok(());
    }
    Err(format!(
        "input {:?}\n     expected:\n{}\n     actual:\n{}",
        case.data, case.document, actual
    ))
}

fn report(summary: &Summary, corpus: &Path, expectations: &Expectations) {
    let executed = summary.total - summary.skipped;
    eprintln!(
        "[tree-construction] corpus={} total={} executed={} passed={} xfailed={} xpassed={} failures={} skipped={}",
        corpus.display(),
        summary.total,
        executed,
        summary.passed,
        summary.xfailed,
        summary.xpassed.len(),
        summary.failures.len(),
        summary.skipped,
    );
    // Conformance counts genuine passes alone. Folding xfailed into the
    // numerator would restate every case the expectations file tolerates as a
    // case the parser handles, which is the metric defect this harness exists
    // to remove. Gate status and conformance are separate lines because they
    // answer different questions: the gate is green whenever no unexpected
    // failure appears, at any conformance rate.
    eprintln!(
        "[tree-construction] conformance={:.2}% of executed, {:.2}% of total",
        ratio(summary.passed, executed) * 100.0,
        ratio(summary.passed, summary.total) * 100.0
    );
    eprintln!(
        "[tree-construction] gate={} ({} unexpected failure(s), {} recorded gap(s))",
        if summary.failures.is_empty() {
            "green"
        } else {
            "red"
        },
        summary.failures.len(),
        summary.xfailed,
    );
    match &expectations.source {
        Some(path) => eprintln!("[tree-construction] expectations={}", path.display()),
        None => eprintln!("[tree-construction] expectations=none (every case must pass)"),
    }
    for (id, message) in &summary.failures {
        eprintln!("[tree-construction] unexpected-failure {id} :: {message}");
    }
    for id in &summary.xpassed {
        eprintln!("[tree-construction] unexpected-pass {id}");
    }
}

/// Emit the dual-denominator scorecard the test262 runner established.
///
/// `pass` counts only genuine passes; `xfailed` cases are recorded failures
/// that the expectations file tolerates, so folding them into `pass` would
/// restate a known gap as conformance.
fn write_scorecard(summary: &Summary) {
    let Ok(raw_path) = env::var("HTML5LIB_TREE_SCORECARD") else {
        return;
    };
    // cargo runs the test with the crate directory as CWD, so a
    // repository-relative path resolves against repo_root rather than CWD.
    let requested = PathBuf::from(&raw_path);
    let path = if requested.is_absolute() {
        requested
    } else {
        repo_root().join(requested)
    };
    let executed = summary.total - summary.skipped;
    let revision = corpus_revision().unwrap_or_else(|| "unknown".to_string());
    let json = format!(
        "{{\n  \"runner\": \"html5lib_tree_construction\",\n  \"runner_kind\": \"wpt-tree-construction\",\n  \"corpus\": \"wpt html/syntax/parsing/resources\",\n  \"corpus_revision\": \"{revision}\",\n  \"total\": {},\n  \"executed\": {},\n  \"pass\": {},\n  \"expected_fail\": {},\n  \"skip\": {},\n  \"rate_executed\": {:.4},\n  \"rate_total\": {:.4}\n}}\n",
        summary.total,
        executed,
        summary.passed,
        summary.xfailed,
        summary.skipped,
        ratio(summary.passed, executed),
        ratio(summary.passed, summary.total),
    );
    if let Err(error) = fs::write(&path, json) {
        eprintln!(
            "[tree-construction] scorecard write failed for {}: {error}",
            path.display()
        );
    }
}

/// Read the pinned corpus revision so a scorecard cannot outlive its corpus.
fn corpus_revision() -> Option<String> {
    let manifest = repo_root().join("silksurf-extras/html-css-test-corpora-revisions.txt");
    let raw = fs::read_to_string(manifest).ok()?;
    raw.lines()
        .find(|line| line.starts_with("wpt-css-parser-subset "))
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
fn dat_parser_splits_cases_and_sections() {
    let raw = "#data\n<p>One\n#errors\n(1,3): boom\n#document\n| <html>\n|   <head>\n|   <body>\n|     <p>\n|       \"One\"\n\n#data\n<b>x\n#document\n| <html>\n";
    let cases = parse_dat_file("sample", raw);
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[0].id, "sample:0");
    assert_eq!(cases[0].data, "<p>One");
    assert!(cases[0].document.starts_with("| <html>"));
    assert!(cases[0].document.ends_with("\"One\""));
    assert_eq!(cases[1].data, "<b>x");
}

#[test]
fn dat_parser_records_fragment_context() {
    let raw = "#data\n<td>x\n#errors\n#document-fragment\ntr\n#document\n| <td>\n";
    let cases = parse_dat_file("frag", raw);
    assert_eq!(cases[0].fragment_context.as_deref(), Some("tr"));
}

#[test]
fn serializer_matches_html5lib_shape() {
    let dom = parse_html("<!DOCTYPE html><p id=\"a\" class=\"b\">One");
    let rendered = serialize_dom(&dom);
    assert_eq!(
        rendered,
        "| <!DOCTYPE html>\n| <html>\n|   <head>\n|   <body>\n|     <p>\n|       class=\"b\"\n|       id=\"a\"\n|       \"One\""
    );
}

#[test]
fn expectations_classify_by_case_id() {
    let config = Expectations::parse(
        "expected-fail template01:*\nskip domjs-unsafe:3\n",
        Path::new("inline"),
    )
    .expect("parse");
    assert_eq!(config.classify("template01:7"), Expected::Fail);
    assert_eq!(config.classify("domjs-unsafe:3"), Expected::Skip);
    assert_eq!(config.classify("tests1:0"), Expected::Pass);
}
